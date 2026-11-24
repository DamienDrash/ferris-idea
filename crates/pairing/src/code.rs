//! The short pairing code of `ADR-0025` §2: 10 characters of Crockford's Base32 (50 bits).

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// A new code from the operating system's random source, or `None` when it fails.
pub fn generate() -> Option<String> {
    let mut b = [0u8; 10];
    getrandom::fill(&mut b).ok()?;
    Some(
        b.iter()
            .map(|x| char::from(ALPHABET[usize::from(x & 0x1f)]))
            .collect(),
    )
}

/// A 256-bit pairing secret for a QR link (`SPEC-0324` behaviour 3), or `None` when the random
/// source fails.
pub fn secret() -> Option<String> {
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).ok()?;
    Some(b.iter().map(|x| format!("{x:02X}")).collect())
}

/// What a QR code or link carries: the address, the host's static key and the secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairLink {
    /// Address to connect to.
    pub address: String,
    /// The host's static public key.
    pub key: [u8; 32],
    /// The pairing secret.
    pub secret: String,
}

/// Writes a pairing link.
pub fn format_link(address: &str, key: &[u8; 32], secret: &str) -> String {
    let hexed: String = key.iter().map(|x| format!("{x:02x}")).collect();
    format!("ferris-idea-pair://v1/{address}?k={hexed}&s={secret}")
}

/// Reads a pairing link; `None` when it is not one.
pub fn parse_link(text: &str) -> Option<PairLink> {
    let rest = text.trim().strip_prefix("ferris-idea-pair://v1/")?;
    let (address, query) = rest.split_once('?')?;
    if address.is_empty() || address.contains('/') {
        return None;
    }
    let mut key = None;
    let mut secret = None;
    for field in query.split('&') {
        match field.split_once('=') {
            Some(("k", v)) if v.len() == 64 => {
                let mut out = [0u8; 32];
                for (i, slot) in out.iter_mut().enumerate() {
                    *slot = u8::from_str_radix(v.get(i * 2..i * 2 + 2)?, 16).ok()?;
                }
                key = Some(out);
            }
            Some(("s", v)) if v.len() >= 32 && v.bytes().all(|b| b.is_ascii_alphanumeric()) => {
                secret = Some(v.to_owned());
            }
            _ => return None,
        }
    }
    Some(PairLink {
        address: address.to_owned(),
        key: key?,
        secret: secret?,
    })
}

/// The canonical form of a typed code: upper case, without separators, with Crockford's
/// look-alikes (`O`→`0`, `I`/`L`→`1`) resolved.
pub fn normalize(code: &str) -> String {
    code.chars()
        .filter(|c| !matches!(c, '-' | ' '))
        .map(|c| match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            other => other,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_have_ten_valid_characters_and_differ() {
        let a = generate().unwrap();
        assert_eq!(a.len(), 10);
        assert!(a.bytes().all(|b| ALPHABET.contains(&b)));
        assert_ne!(a, generate().unwrap());
    }

    #[test]
    fn a_link_round_trips_and_bad_ones_are_refused() {
        let key = [7u8; 32];
        let secret = secret().unwrap();
        assert_eq!(secret.len(), 64);
        let link = format_link("host:47100", &key, &secret);
        let parsed = parse_link(&link).unwrap();
        assert_eq!(parsed.address, "host:47100");
        assert_eq!(parsed.key, key);
        assert_eq!(parsed.secret, secret);
        assert!(parse_link("ferris-idea-pair://v1/host:1?k=zz&s=abc").is_none());
        assert!(parse_link("https://x/?k=00&s=00").is_none());
    }

    #[test]
    fn typing_is_forgiving() {
        assert_eq!(normalize("abcd-ol i 23"), "ABCD01123");
    }
}
