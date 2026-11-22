//! 使用 AES-256-GCM 的凭据加密（docs/05 §4）。

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use rand::Rng;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CryptoError {
    #[error("Invalid master key: {0}")]
    InvalidKey(String),
    #[error("Encryption failed: {0}")]
    EncryptionFailed(String),
    #[error("Decryption failed: {0}")]
    DecryptionFailed(String),
}

pub struct CredentialCrypto {
    cipher: Aes256Gcm,
}

impl CredentialCrypto {
    /// 以 base64 编码的 32 字节主密钥初始化。
    pub fn new(master_key_b64: &str) -> Result<Self, CryptoError> {
        let key_bytes = BASE64
            .decode(master_key_b64)
            .map_err(|e| CryptoError::InvalidKey(format!("base64 decode: {}", e)))?;

        if key_bytes.len() != 32 {
            return Err(CryptoError::InvalidKey(format!(
                "expected 32 bytes, got {}",
                key_bytes.len()
            )));
        }

        let cipher = Aes256Gcm::new_from_slice(&key_bytes)
            .map_err(|e| CryptoError::InvalidKey(format!("AES init: {}", e)))?;

        Ok(Self { cipher })
    }

    /// 加密明文并返回 base64(nonce[12] || ciphertext)。
    /// 每次加密严格使用安全的随机 12 字节 Nonce，杜绝 AES-GCM Nonce 重用漏洞。
    pub fn encrypt(&self, plaintext: &str) -> Result<String, CryptoError> {
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = self
            .cipher
            .encrypt(nonce, plaintext.as_bytes())
            .map_err(|e| CryptoError::EncryptionFailed(e.to_string()))?;

        let mut result = Vec::with_capacity(12 + ciphertext.len());
        result.extend_from_slice(&nonce_bytes);
        result.extend_from_slice(&ciphertext);
        Ok(BASE64.encode(&result))
    }

    /// 解密 base64(nonce || ciphertext) 并返回明文。
    pub fn decrypt(&self, ciphertext_b64: &str) -> Result<String, CryptoError> {
        let data = BASE64
            .decode(ciphertext_b64)
            .map_err(|e| CryptoError::DecryptionFailed(format!("base64: {}", e)))?;

        if data.len() < 12 {
            return Err(CryptoError::DecryptionFailed("data too short".to_string()));
        }

        let (nonce_bytes, ciphertext) = data.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);

        let plaintext = self
            .cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| CryptoError::DecryptionFailed(e.to_string()))?;

        String::from_utf8(plaintext)
            .map_err(|e| CryptoError::DecryptionFailed(format!("utf8: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key() -> String {
        // 恰好 32 字节
        BASE64.encode(b"12345678901234567890123456789012")
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let crypto = CredentialCrypto::new(&test_key()).unwrap();
        let plaintext = "exnsk-f85dd8db1234567890abcdef";
        let encrypted = crypto.encrypt(plaintext).unwrap();
        let decrypted = crypto.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn invalid_key_length() {
        let short_key = BASE64.encode(b"short");
        let result = CredentialCrypto::new(&short_key);
        assert!(result.is_err());
    }
}
