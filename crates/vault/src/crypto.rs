//! The primitives of the vault (`SPEC-0353` behaviour 5): XChaCha20-Poly1305 with random 192-bit
//! nonces, HKDF-SHA-256 for the derived keys, HMAC-SHA-256 over the file.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use data_encoding::HEXLOWER;
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::{EntryMeta, Keys, StoredEntry, VaultError, VaultFile};

const WRAP_INFO: &[u8] = b"idecraft-vault/v1/wrap"; // rebrand:keep (key derivation label of existing vaults)
const MAC_INFO: &[u8] = b"idecraft-vault/v1/mac"; // rebrand:keep (key derivation label of existing vaults)
const AUDIT_INFO: &[u8] = b"idecraft-vault/v1/audit"; // rebrand:keep (key derivation label of existing vaults)

/// `N` bytes from the operating system's random source.
pub(crate) fn random<const N: usize>() -> Result<[u8; N], VaultError> {
    let mut out = [0u8; N];
    getrandom::fill(&mut out)
        .map_err(|e| VaultError::Io(format!("the system's random source failed: {e}")))?;
    Ok(out)
}

/// A key derived from the vault key for `info`.
fn derive(keys: &Keys, info: &[u8]) -> Zeroizing<[u8; 32]> {
    let hk = Hkdf::<Sha256>::new(Some(&keys.vault_id), keys.vk.as_ref());
    let mut out = Zeroizing::new([0u8; 32]);
    // 32 bytes are always within HKDF-SHA-256's limit.
    let _ = hk.expand(info, out.as_mut());
    out
}

/// The key of the audit log's MACs (behaviour 20).
pub(crate) fn audit_key(keys: &Keys) -> Zeroizing<[u8; 32]> {
    derive(keys, AUDIT_INFO)
}

/// HMAC-SHA-256 of `parts` under `key`, hex.
pub(crate) fn mac_hex(key: &[u8], parts: &[&[u8]]) -> String {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("HMAC takes any key length");
    for p in parts {
        mac.update(p);
    }
    HEXLOWER.encode(&mac.finalize().into_bytes())
}

/// Whether `tag` (hex) is the HMAC of `parts`, compared in constant time.
pub(crate) fn mac_ok(key: &[u8], parts: &[&[u8]], tag: &str) -> bool {
    let Ok(tag) = HEXLOWER.decode(tag.as_bytes()) else {
        return false;
    };
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("HMAC takes any key length");
    for p in parts {
        mac.update(p);
    }
    mac.verify_slice(&tag).is_ok()
}

fn aad(keys: &Keys, meta: &EntryMeta, field: &str) -> Vec<u8> {
    format!(
        "idecraft-vault/v1|{}|{}|{}|{field}", // rebrand:keep (associated data of existing vaults)
        HEXLOWER.encode(&keys.vault_id),
        meta.handle,
        meta.version
    )
    .into_bytes()
}

fn seal(key: &[u8; 32], msg: &[u8], aad: &[u8]) -> Result<(String, String), VaultError> {
    let nonce = random::<24>()?;
    let ct = XChaCha20Poly1305::new(Key::from_slice(key))
        .encrypt(XNonce::from_slice(&nonce), Payload { msg, aad })
        .map_err(|_| VaultError::Io("encryption failed".into()))?;
    Ok((HEXLOWER.encode(&nonce), HEXLOWER.encode(&ct)))
}

fn open(
    key: &[u8; 32],
    nonce: &str,
    ct: &str,
    aad: &[u8],
) -> Result<Zeroizing<Vec<u8>>, VaultError> {
    let bad =
        || VaultError::Corrupt("an entry does not decrypt; the vault file was changed".into());
    let nonce = HEXLOWER.decode(nonce.as_bytes()).map_err(|_| bad())?;
    let ct = HEXLOWER.decode(ct.as_bytes()).map_err(|_| bad())?;
    if nonce.len() != 24 {
        return Err(bad());
    }
    XChaCha20Poly1305::new(Key::from_slice(key))
        .decrypt(XNonce::from_slice(&nonce), Payload { msg: &ct, aad })
        .map(Zeroizing::new)
        .map_err(|_| bad())
}

/// `value` encrypted under a new data key, the data key wrapped by the vault key.
pub(crate) fn seal_entry(
    keys: &Keys,
    meta: EntryMeta,
    value: &str,
) -> Result<StoredEntry, VaultError> {
    let dk = Zeroizing::new(random::<32>()?);
    let wrap = derive(keys, WRAP_INFO);
    let (key_nonce, key) = seal(&wrap, dk.as_ref(), &aad(keys, &meta, "key"))?;
    let (value_nonce, value) = seal(&dk, value.as_bytes(), &aad(keys, &meta, "value"))?;
    Ok(StoredEntry {
        meta,
        key_nonce,
        key,
        value_nonce,
        value,
    })
}

/// The value of `entry`.
pub(crate) fn open_entry(
    keys: &Keys,
    entry: &StoredEntry,
) -> Result<Zeroizing<String>, VaultError> {
    let wrap = derive(keys, WRAP_INFO);
    let dk_bytes = open(
        &wrap,
        &entry.key_nonce,
        &entry.key,
        &aad(keys, &entry.meta, "key"),
    )?;
    if dk_bytes.len() != 32 {
        return Err(VaultError::Corrupt(
            "an entry's data key is malformed".into(),
        ));
    }
    let mut dk = Zeroizing::new([0u8; 32]);
    dk.copy_from_slice(&dk_bytes);
    let plain = open(
        &dk,
        &entry.value_nonce,
        &entry.value,
        &aad(keys, &entry.meta, "value"),
    )?;
    String::from_utf8(plain.to_vec())
        .map(Zeroizing::new)
        .map_err(|_| VaultError::Corrupt("an entry's value is not UTF-8".into()))
}

fn file_bytes(file: &VaultFile) -> Result<Vec<u8>, VaultError> {
    let mut unsigned = file.clone();
    unsigned.mac = String::new();
    serde_json::to_vec(&unsigned).map_err(|e| VaultError::Io(e.to_string()))
}

/// The MAC of `file` (behaviour 5).
pub(crate) fn file_mac(keys: &Keys, file: &VaultFile) -> Result<String, VaultError> {
    Ok(mac_hex(
        derive(keys, MAC_INFO).as_ref(),
        &[&file_bytes(file)?],
    ))
}

/// Refuses a file whose vault id, format or MAC does not fit the key (behaviour 6).
pub(crate) fn verify_file(keys: &Keys, file: &VaultFile) -> Result<(), VaultError> {
    if file.format != crate::FORMAT {
        return Err(VaultError::Corrupt(format!(
            "the vault file has format {}, this Ferris IDEA reads {}",
            file.format,
            crate::FORMAT
        )));
    }
    if file.vault_id != HEXLOWER.encode(&keys.vault_id) {
        return Err(VaultError::Corrupt(
            "the vault file belongs to another key".into(),
        ));
    }
    if !mac_ok(
        derive(keys, MAC_INFO).as_ref(),
        &[&file_bytes(file)?],
        &file.mac,
    ) {
        return Err(VaultError::Corrupt(
            "the vault file's authentication failed; it was changed outside Ferris IDEA".into(),
        ));
    }
    Ok(())
}
