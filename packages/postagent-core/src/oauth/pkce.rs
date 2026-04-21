use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use sha2::{Digest, Sha256};

pub struct PkceCodes {
    pub verifier: String,
    pub challenge: String,
}

/// S256 PKCE (RFC 7636). 48 random bytes → base64url-no-pad verifier; SHA256
/// of the verifier bytes (i.e. the ASCII base64url-no-pad string) → challenge.
pub fn generate() -> PkceCodes {
    let mut bytes = [0u8; 48];
    rand::thread_rng().fill_bytes(&mut bytes);
    let verifier = URL_SAFE_NO_PAD.encode(bytes);

    // Per RFC 7636 §4.2, the challenge hashes the ASCII bytes of the verifier
    // string, not the original random bytes.
    let digest = Sha256::digest(verifier.as_bytes());
    let challenge = URL_SAFE_NO_PAD.encode(digest);

    PkceCodes {
        verifier,
        challenge,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifier_uses_rfc_allowed_characters() {
        let c = generate();
        // RFC 7636: code-verifier = 43*128unreserved
        // base64url-no-pad produces [A-Za-z0-9_-] which is a subset of unreserved.
        assert!(!c.verifier.is_empty());
        for ch in c.verifier.chars() {
            assert!(
                ch.is_ascii_alphanumeric() || ch == '-' || ch == '_',
                "verifier contains disallowed char: {}",
                ch
            );
        }
        // 48 bytes → 64 chars base64url-no-pad.
        assert_eq!(c.verifier.len(), 64);
    }

    #[test]
    fn challenge_is_sha256_of_verifier_base64url() {
        let c = generate();
        let expected = URL_SAFE_NO_PAD.encode(Sha256::digest(c.verifier.as_bytes()));
        assert_eq!(c.challenge, expected);
        // SHA256 → 32 bytes → 43 chars base64url-no-pad.
        assert_eq!(c.challenge.len(), 43);
    }

    #[test]
    fn distinct_calls_produce_distinct_verifiers() {
        let a = generate();
        let b = generate();
        assert_ne!(a.verifier, b.verifier);
    }
}
