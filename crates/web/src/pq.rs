//! The hybrid key exchange `X25519MLKEM768` (`draft-ietf-tls-ecdhe-mlkem`, group `0x11EC`) as a
//! `rustls` key exchange group (`SPEC-0325` behaviour 4): ML-KEM-768 from `libcrux-ml-kem`, X25519
//! from `ring`. Shares and secret are concatenated with the ML-KEM part first.

use libcrux_ml_kem::mlkem768 as kem;
use ring::agreement::{self, EphemeralPrivateKey, UnparsedPublicKey, X25519};
use ring::rand::{SecureRandom, SystemRandom};
use rustls::crypto::{ActiveKeyExchange, CompletedKeyExchange, SharedSecret, SupportedKxGroup};
use rustls::{Error, NamedGroup, PeerMisbehaved};
use zeroize::{Zeroize, Zeroizing};

const X25519_LEN: usize = 32;
const EK_LEN: usize = 1184;
const CT_LEN: usize = 1088;
const SS_LEN: usize = 32;

/// The group as `rustls` takes it.
pub static X25519_MLKEM768: &dyn SupportedKxGroup = &Group;

fn invalid_share() -> Error {
    Error::PeerMisbehaved(PeerMisbehaved::InvalidKeyShare)
}

fn no_random() -> Error {
    Error::General("no random numbers".into())
}

fn random<const N: usize>() -> Result<Zeroizing<[u8; N]>, Error> {
    let mut bytes = Zeroizing::new([0u8; N]);
    SystemRandom::new()
        .fill(bytes.as_mut())
        .map_err(|_| no_random())?;
    Ok(bytes)
}

fn x25519_pair() -> Result<(EphemeralPrivateKey, Vec<u8>), Error> {
    let private =
        EphemeralPrivateKey::generate(&X25519, &SystemRandom::new()).map_err(|_| no_random())?;
    let public = private
        .compute_public_key()
        .map_err(|_| Error::General("X25519 failed".into()))?
        .as_ref()
        .to_vec();
    Ok((private, public))
}

fn x25519_agree(private: EphemeralPrivateKey, peer: &[u8]) -> Result<Vec<u8>, Error> {
    agreement::agree_ephemeral(private, &UnparsedPublicKey::new(&X25519, peer), |s| {
        s.to_vec()
    })
    .map_err(|_| invalid_share())
}

fn secret(mlkem: &[u8], x25519: &mut Vec<u8>) -> SharedSecret {
    let mut joined = Vec::with_capacity(SS_LEN + X25519_LEN);
    joined.extend_from_slice(mlkem);
    joined.extend_from_slice(x25519);
    x25519.zeroize();
    SharedSecret::from(joined)
}

#[derive(Debug)]
struct Group;

impl SupportedKxGroup for Group {
    fn start(&self) -> Result<Box<dyn ActiveKeyExchange>, Error> {
        let seed = random::<64>()?;
        let pair = kem::generate_key_pair(*seed);
        let (x_private, x_public) = x25519_pair()?;
        let mut share = Vec::with_capacity(EK_LEN + X25519_LEN);
        share.extend_from_slice(pair.pk());
        share.extend_from_slice(&x_public);
        Ok(Box::new(Active {
            pair,
            x25519: x_private,
            share,
        }))
    }

    fn start_and_complete(&self, client: &[u8]) -> Result<CompletedKeyExchange, Error> {
        if client.len() != EK_LEN + X25519_LEN {
            return Err(invalid_share());
        }
        let (ek, x_peer) = client.split_at(EK_LEN);
        let ek: [u8; EK_LEN] = ek.try_into().map_err(|_| invalid_share())?;
        let ek = kem::MlKem768PublicKey::from(ek);
        // FIPS 203 requires this check before encapsulating to a key from outside.
        if !kem::validate_public_key(&ek) {
            return Err(invalid_share());
        }
        let randomness = random::<32>()?;
        let (ct, mlkem) = kem::encapsulate(&ek, *randomness);
        let mlkem = Zeroizing::new(mlkem);
        let (x_private, x_public) = x25519_pair()?;
        let mut x_secret = x25519_agree(x_private, x_peer)?;
        let mut pub_key = Vec::with_capacity(CT_LEN + X25519_LEN);
        pub_key.extend_from_slice(ct.as_slice());
        pub_key.extend_from_slice(&x_public);
        Ok(CompletedKeyExchange {
            group: self.name(),
            pub_key,
            secret: secret(mlkem.as_slice(), &mut x_secret),
        })
    }

    fn ffdhe_group(&self) -> Option<rustls::ffdhe_groups::FfdheGroup<'static>> {
        None
    }

    fn name(&self) -> NamedGroup {
        NamedGroup::X25519MLKEM768
    }

    fn usable_for_version(&self, version: rustls::ProtocolVersion) -> bool {
        version == rustls::ProtocolVersion::TLSv1_3
    }
}

struct Active {
    pair: kem::MlKem768KeyPair,
    x25519: EphemeralPrivateKey,
    share: Vec<u8>,
}

impl ActiveKeyExchange for Active {
    fn complete(self: Box<Self>, server: &[u8]) -> Result<SharedSecret, Error> {
        if server.len() != CT_LEN + X25519_LEN {
            return Err(invalid_share());
        }
        let (ct, x_peer) = server.split_at(CT_LEN);
        let ct: [u8; CT_LEN] = ct.try_into().map_err(|_| invalid_share())?;
        let ct = kem::MlKem768Ciphertext::from(ct);
        let mlkem = Zeroizing::new(kem::decapsulate(self.pair.private_key(), &ct));
        let mut x_secret = x25519_agree(self.x25519, x_peer)?;
        Ok(secret(mlkem.as_slice(), &mut x_secret))
    }

    fn pub_key(&self) -> &[u8] {
        &self.share
    }

    fn group(&self) -> NamedGroup {
        NamedGroup::X25519MLKEM768
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ml_kem::kem::{Decapsulate, Encapsulate, KeyExport, TryKeyInit};
    use ml_kem::{Seed, ml_kem_768};

    #[test]
    fn ak2_client_and_server_halves_agree() {
        let client = X25519_MLKEM768.start().unwrap();
        let done = X25519_MLKEM768
            .start_and_complete(client.pub_key())
            .unwrap();
        assert_eq!(done.pub_key.len(), CT_LEN + X25519_LEN);
        assert_eq!(client.pub_key().len(), EK_LEN + X25519_LEN);
        let server = done.secret;
        let client = client.complete(&done.pub_key).unwrap();
        assert_eq!(client.secret_bytes(), server.secret_bytes());
        assert_eq!(client.secret_bytes().len(), SS_LEN + X25519_LEN);
    }

    #[test]
    fn ak2_malformed_shares_are_refused() {
        assert!(X25519_MLKEM768.start_and_complete(&[0u8; 12]).is_err());
        // An encapsulation key whose coefficients are out of range fails the FIPS 203 check.
        let bad = vec![0xffu8; EK_LEN + X25519_LEN];
        assert!(X25519_MLKEM768.start_and_complete(&bad).is_err());
        let client = X25519_MLKEM768.start().unwrap();
        assert!(client.complete(&[1, 2, 3]).is_err());
    }

    #[test]
    fn ak2_a_low_order_x25519_point_is_refused() {
        let client = X25519_MLKEM768.start().unwrap();
        let mut share = client.pub_key().to_vec();
        for b in &mut share[EK_LEN..] {
            *b = 0;
        }
        assert!(X25519_MLKEM768.start_and_complete(&share).is_err());
    }

    #[test]
    fn ak2_the_mlkem_half_matches_a_second_implementation() {
        // RustCrypto's ML-KEM-768 decapsulates what this group encapsulated to its key.
        let dk = ml_kem_768::DecapsulationKey::from_seed(Seed::from([21u8; 64]));
        let mut share = dk.encapsulation_key().to_bytes().as_slice().to_vec();
        let (_, x_public) = x25519_pair().unwrap();
        share.extend_from_slice(&x_public);
        let done = X25519_MLKEM768.start_and_complete(&share).unwrap();
        let theirs = dk.decapsulate_slice(&done.pub_key[..CT_LEN]).unwrap();
        assert_eq!(&done.secret.secret_bytes()[..SS_LEN], theirs.as_slice());

        // And the other direction: libcrux decapsulates what RustCrypto encapsulated.
        let client = X25519_MLKEM768.start().unwrap();
        let ek = ml_kem_768::EncapsulationKey::new_from_slice(&client.pub_key()[..EK_LEN]).unwrap();
        let (ct, ours) = ek.encapsulate();
        let (_, x_public) = x25519_pair().unwrap();
        let mut server_share = ct.as_slice().to_vec();
        server_share.extend_from_slice(&x_public);
        let got = client.complete(&server_share).unwrap();
        assert_eq!(&got.secret_bytes()[..SS_LEN], ours.as_slice());
    }
}
