//! Signature checks: Nextcloud signs with the user's key, RSA PKCS#1 v1.5 with SHA-512
//! (`openssl_sign(..., OPENSSL_ALGO_SHA512)`), base64 encoded.

use base64::Engine as _;
use rsa::pkcs1v15::{Signature, VerifyingKey};
use rsa::pkcs8::DecodePublicKey;
use rsa::signature::Verifier;
use rsa::RsaPublicKey;
use sha2::Sha512;

pub fn signature(public_key_pem: &str, data: &[u8], signature_b64: &str) -> bool {
    let Ok(key) = RsaPublicKey::from_public_key_pem(public_key_pem.trim())
        .or_else(|_| <RsaPublicKey as rsa::pkcs1::DecodeRsaPublicKey>::from_pkcs1_pem(public_key_pem.trim()))
    else {
        return false;
    };
    let Ok(raw) = base64::engine::general_purpose::STANDARD.decode(signature_b64.trim()) else {
        return false;
    };
    let Ok(sig) = Signature::try_from(raw.as_slice()) else {
        return false;
    };
    VerifyingKey::<Sha512>::new(key).verify(data, &sig).is_ok()
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
    }

    fn rand_core_compat() -> rsa::rand_core::OsRng {
        rsa::rand_core::OsRng
    }
}
