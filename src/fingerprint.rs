//! Phase 4: canonical serialization + cache keys.
//!
//! key(f) = BLAKE3( schema_version | compiler_version | target | options | body | deps... )
//! Every field is length-prefixed so ("ab","c") and ("a","bc") hash differently.

/// Bump whenever the canonical serialization changes.
pub const SCHEMA_VERSION: u32 = 1;

/// 256-bit cache key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Key(pub [u8; 32]);

impl Key {
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Inverse of `to_hex`; `None` unless the string is exactly 64 hex chars.
    pub fn from_hex(s: &str) -> Option<Key> {
        let chars: Vec<char> = s.chars().collect();
        if chars.len() != 64 { return None; }
        let mut bytes = [0u8; 32];
        for (i, pair) in chars.chunks(2).enumerate() {
            let hi = pair[0].to_digit(16)?;
            let lo = pair[1].to_digit(16)?;
            bytes[i] = ((hi << 4) | lo) as u8;
        }
        Some(Key(bytes))
    }
}

pub struct KeyBuilder {
    hasher: blake3::Hasher,
}

impl KeyBuilder {
    pub fn new(compiler_version: &str, target: &str, options: &str) -> Self {
        let mut kb = Self { hasher: blake3::Hasher::new() };
        kb.hasher.update(&SCHEMA_VERSION.to_le_bytes());
        kb.field(compiler_version.as_bytes());
        kb.field(target.as_bytes());
        kb.field(options.as_bytes());
        kb
    }

    /// Append one length-prefixed field (canonical body or dependency encoding).
    /// Callers must add dependencies in a canonical (sorted) order.
    pub fn field(&mut self, bytes: &[u8]) -> &mut Self {
        self.hasher.update(&(bytes.len() as u64).to_le_bytes());
        self.hasher.update(bytes);
        self
    }

    pub fn finish(&self) -> Key {
        Key(*self.hasher.finalize().as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(fields: &[&str]) -> Key {
        let mut kb = KeyBuilder::new("0.1", "x86_64", "-O1");
        for f in fields {
            kb.field(f.as_bytes());
        }
        kb.finish()
    }

    #[test]
    fn deterministic() {
        assert_eq!(key(&["body", "dep"]), key(&["body", "dep"]));
    }

    #[test]
    fn length_prefix_prevents_ambiguity() {
        assert_ne!(key(&["ab", "c"]), key(&["a", "bc"]));
    }

    #[test]
    fn options_change_key() {
        let a = KeyBuilder::new("0.1", "x86_64", "-O1").finish();
        let b = KeyBuilder::new("0.1", "x86_64", "-O2").finish();
        assert_ne!(a, b);
    }

    #[test]
    fn hex_is_64_chars() {
        assert_eq!(key(&["x"]).to_hex().len(), 64);
    }
}
