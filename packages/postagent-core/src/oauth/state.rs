use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;

/// 32 random bytes → base64url-no-pad. Used as the OAuth `state` parameter.
pub fn generate() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Constant-time comparison. Returns true iff both inputs are equal byte-wise.
pub fn equals(a: &str, b: &str) -> bool {
    let a = a.as_bytes();
    let b = b.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_state_is_reasonable_length() {
        let s = generate();
        // 32 bytes → 43 chars base64url-no-pad.
        assert_eq!(s.len(), 43);
    }

    #[test]
    fn equals_matches() {
        assert!(equals("abc", "abc"));
        assert!(!equals("abc", "abd"));
        assert!(!equals("abc", "ab"));
        assert!(equals("", ""));
    }
}
