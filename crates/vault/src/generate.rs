//! Values generated inside the vault (`SPEC-0353` behaviour 16): uniform choice from the
//! operating system's random source by rejection sampling.

use zeroize::Zeroizing;

use crate::VaultError;

/// The characters a generated value is drawn from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Charset {
    /// The 94 printable ASCII characters (`!` to `~`).
    #[default]
    Printable,
    /// `A-Z`, `a-z`, `0-9`.
    Alphanumeric,
}

impl Charset {
    /// The charset named `name`.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "printable" => Some(Charset::Printable),
            "alphanumeric" => Some(Charset::Alphanumeric),
            _ => None,
        }
    }

    fn alphabet(self) -> Vec<u8> {
        match self {
            Charset::Printable => (b'!'..=b'~').collect(),
            Charset::Alphanumeric => (b'A'..=b'Z')
                .chain(b'a'..=b'z')
                .chain(b'0'..=b'9')
                .collect(),
        }
    }
}

/// Length and characters of a generated value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    /// 16 to 128 characters.
    pub length: usize,
    /// The characters.
    pub charset: Charset,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            length: 24,
            charset: Charset::Printable,
        }
    }
}

/// Shortest and longest generated value.
pub const LENGTH_BOUNDS: (usize, usize) = (16, 128);

/// A value following `policy`; never containing `${`, which a run would expand.
pub fn generate(policy: Policy) -> Result<Zeroizing<String>, VaultError> {
    let (min, max) = LENGTH_BOUNDS;
    if !(min..=max).contains(&policy.length) {
        return Err(VaultError::Invalid(format!("length is {min} to {max}")));
    }
    let alphabet = policy.charset.alphabet();
    let n = alphabet.len();
    // The largest multiple of n below 256: bytes at or above it are drawn again, so every
    // character is equally likely.
    let limit = 256 - 256 % n;
    loop {
        let mut out = Zeroizing::new(String::with_capacity(policy.length));
        while out.len() < policy.length {
            let mut buf = Zeroizing::new([0u8; 64]);
            getrandom::fill(buf.as_mut())
                .map_err(|e| VaultError::Io(format!("the system's random source failed: {e}")))?;
            for b in buf.iter().map(|b| *b as usize) {
                if b < limit && out.len() < policy.length {
                    out.push(alphabet[b % n] as char);
                }
            }
        }
        if !out.contains("${") {
            return Ok(out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SPEC-0353 AK-2: length, characters and no `${`.
    #[test]
    fn generated_values_follow_the_policy() {
        for _ in 0..200 {
            let v = generate(Policy::default()).unwrap();
            assert_eq!(v.len(), 24);
            assert!(v.bytes().all(|b| (b'!'..=b'~').contains(&b)));
            assert!(!v.contains("${"));
        }
        let a = generate(Policy {
            length: 128,
            charset: Charset::Alphanumeric,
        })
        .unwrap();
        assert_eq!(a.len(), 128);
        assert!(a.bytes().all(|b| b.is_ascii_alphanumeric()));
        assert_ne!(
            generate(Policy::default()).unwrap(),
            generate(Policy::default()).unwrap()
        );
        for length in [15, 129] {
            let p = Policy {
                length,
                ..Policy::default()
            };
            assert!(generate(p).is_err());
        }
    }
}
