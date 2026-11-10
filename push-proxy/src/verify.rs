//! Signature checks: Nextcloud signs with the user's key, RSA PKCS#1 v1.5 with SHA-512
//! (`openssl_sign(..., OPENSSL_ALGO_SHA512)`), base64 encoded.

use base64::Engine as _;
use rsa::pkcs1v15::{Signature, VerifyingKey};
use rsa::pkcs8::DecodePublicKey;
use rsa::signature::Verifier;
use rsa::signature::hazmat::PrehashVerifier;
use rsa::RsaPublicKey;
use sha2::Sha512;

fn parts(public_key_pem: &str, signature_b64: &str) -> Option<(VerifyingKey<Sha512>, Signature)> {
    let key = RsaPublicKey::from_public_key_pem(public_key_pem.trim())
        .or_else(|_| <RsaPublicKey as rsa::pkcs1::DecodeRsaPublicKey>::from_pkcs1_pem(public_key_pem.trim()))
        .ok()?;
    let raw = b64(signature_b64)?;
    let sig = Signature::try_from(raw.as_slice()).ok()?;
    Some((VerifyingKey::<Sha512>::new(key), sig))
}

pub fn b64(s: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::STANDARD.decode(s.trim()).ok()
}

/// Signature over `data` (notifications: the raw encrypted subject bytes).
pub fn signature(public_key_pem: &str, data: &[u8], signature_b64: &str) -> bool {
    parts(public_key_pem, signature_b64).is_some_and(|(k, s)| k.verify(data, &s).is_ok())
}

/// Device registration: the server signs the identifier *before* hashing it, and the
/// `deviceIdentifier` it hands out is base64(SHA-512(that)). So the identifier is the
/// signed digest itself.
pub fn device_signature(public_key_pem: &str, device_identifier: &str, signature_b64: &str) -> bool {
    let Some(digest) = b64(device_identifier).filter(|d| d.len() == 64) else {
        return false;
    };
    parts(public_key_pem, signature_b64).is_some_and(|(k, s)| k.verify_prehash(&digest, &s).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsa::pkcs1v15::SigningKey;
    use rsa::pkcs8::{EncodePublicKey, LineEnding};
    use rsa::signature::{SignatureEncoding, Signer};
    use rsa::RsaPrivateKey;

    #[test]
    fn verifies_nextcloud_style_signatures() {
        let mut rng = rand_core_compat();
        let private = RsaPrivateKey::new(&mut rng, 2048).unwrap();
        let pem = private.to_public_key().to_public_key_pem(LineEnding::LF).unwrap();
        let sig = SigningKey::<Sha512>::new(private).sign(b"device-123");
        let b64 = base64::engine::general_purpose::STANDARD.encode(sig.to_bytes());
        assert!(signature(&pem, b"device-123", &b64));
        assert!(!signature(&pem, b"device-124", &b64));
        assert!(!signature(&pem, b"device-123", "not base64!"));
        assert!(!signature("garbage", b"device-123", &b64));
        // Registration: identifier = base64(sha512(raw)), signature over raw.
        use sha2::Digest;
        let id = base64::engine::general_purpose::STANDARD.encode(Sha512::digest(b"device-123"));
        assert!(device_signature(&pem, &id, &b64));
        assert!(!device_signature(&pem, "short", &b64));
    }

    fn rand_core_compat() -> rsa::rand_core::OsRng {
        rsa::rand_core::OsRng
    }
}
