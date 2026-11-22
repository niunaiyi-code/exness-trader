//! Exness Trader API 的 Ed25519 请求签名（签名版本 1）。
//!
//! 每个请求携带六个请求头：
//!
//! | 请求头 | 值 |
//! | --- | --- |
//! | `EXN-API-KEY` | 密钥 ID（`exnsk-…`） |
//! | `EXN-TIMESTAMP` | Unix 毫秒时间戳 |
//! | `EXN-SIGN-VERSION` | 恒为 `1` |
//! | `EXN-IDEMPOTENCY-KEY` | 变更类调用需 5..=128 个字符，字符集 `[A-Za-z0-9._~-]`；GET 时**存在但为空** |
//! | `EXN-DATA` | JSON 载荷的 base64url（无填充） |
//! | `EXN-SIGN` | 对 *解码后* 载荷字节的 Ed25519 签名，base64url（无填充） |
//!
//! 载荷为 `{api_key, idempotency_key, timestamp, sign_version, method, path, body_hash}`，其中
//! `path` 是实际发送的请求路径加查询串，`body_hash` 是对应的 base64url
//! 原始请求体的 SHA-256（空请求体使用 `EMPTY_BODY_HASH`）。

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ed25519_dalek::pkcs8::{DecodePrivateKey, EncodePublicKey};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey, SECRET_KEY_LENGTH};
use serde::Serialize;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

/// 签名方案版本，写入 `EXN-SIGN-VERSION` 及载荷。
pub const SIGN_VERSION: u8 = 1;

/// 对零字节做 SHA-256 的 base64url（无填充）。用于 GET / 空 body 请求。
pub const EMPTY_BODY_HASH: &str = "47DEQpj8HBSa-_TImW-5JCeuQeRkm5NMpJWZG3hSuFU";

pub const HEADER_API_KEY: &str = "EXN-API-KEY";
pub const HEADER_TIMESTAMP: &str = "EXN-TIMESTAMP";
pub const HEADER_SIGN_VERSION: &str = "EXN-SIGN-VERSION";
pub const HEADER_IDEMPOTENCY_KEY: &str = "EXN-IDEMPOTENCY-KEY";
pub const HEADER_DATA: &str = "EXN-DATA";
pub const HEADER_SIGN: &str = "EXN-SIGN";

pub const IDEMPOTENCY_KEY_MIN: usize = 5;
pub const IDEMPOTENCY_KEY_MAX: usize = 128;

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("private key is neither 64/128 hex characters nor a PEM block")]
    UnrecognizedPrivateKeyFormat,
    #[error("private key hex is invalid: {0}")]
    InvalidHex(#[from] hex::FromHexError),
    #[error("private key PEM is invalid: {0}")]
    InvalidPem(String),
    #[error("128-hex private key: trailing public key does not match the key derived from the seed")]
    PublicKeyMismatch,
    #[error("api key must not be empty")]
    EmptyApiKey,
    #[error("idempotency key must be {IDEMPOTENCY_KEY_MIN}..={IDEMPOTENCY_KEY_MAX} chars of [A-Za-z0-9._~-]: {0:?}")]
    InvalidIdempotencyKey(String),
}

/// API 密钥 ID + Ed25519 签名密钥。签名密钥在 drop 时清零。
pub struct Credentials {
    api_key: String,
    signing_key: SigningKey,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("api_key", &self.api_key)
            .field("public_key", &self.public_key_hex())
            .finish_non_exhaustive()
    }
}

impl Credentials {
    /// 由密钥 ID 和以下三种格式之一的私钥构建凭据：
    ///
    /// * 128 个十六进制字符：Exness 生成的 `seed ‖ public_key`；会校验公钥部分。
    /// * 64 个十六进制字符：原始 32 字节种子。
    /// * PEM（`-----BEGIN PRIVATE KEY-----`）：用 OpenSSL 生成的 PKCS#8 Ed25519 密钥。
    pub fn new(api_key: impl Into<String>, private_key: &str) -> Result<Self, AuthError> {
        let api_key = api_key.into();
        if api_key.trim().is_empty() {
            return Err(AuthError::EmptyApiKey);
        }
        let signing_key = parse_private_key(private_key)?;
        Ok(Self { api_key, signing_key })
    }

    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    pub fn verifying_key(&self) -> VerifyingKey {
        self.signing_key.verifying_key()
    }

    /// 小写十六进制公钥（32 字节），便于日志与密钥管理页面使用。
    pub fn public_key_hex(&self) -> String {
        hex::encode(self.verifying_key().as_bytes())
    }

    /// 单行 base64 SPKI/DER 公钥，即密钥管理对自生成密钥所要求的格式。
    pub fn public_key_spki_base64(&self) -> Result<String, AuthError> {
        let der = self.verifying_key()
            .to_public_key_der()
            .map_err(|e| AuthError::InvalidPem(format!("SPKI encoding failed: {}", e)))?;
        Ok(base64::engine::general_purpose::STANDARD.encode(der.as_bytes()))
    }

    /// 为单个请求签名。注入 `timestamp_ms` 使测试可复现；生产代码
    /// 传入 [`now_ms`]。
    pub fn sign(&self, req: &SignRequest<'_>) -> Result<SignedHeaders, AuthError> {
        let idempotency_key = match req.idempotency_key {
            Some(k) => {
                validate_idempotency_key(k)?;
                k.to_owned()
            }
            None => String::new(),
        };
        let payload = Payload {
            api_key: &self.api_key,
            idempotency_key: &idempotency_key,
            timestamp: req.timestamp_ms,
            sign_version: SIGN_VERSION,
            method: req.method,
            path: req.path_and_query,
            body_hash: &body_hash(req.body),
        };
        let payload_bytes = serde_json::to_vec(&payload)
            .map_err(|e| AuthError::InvalidPem(format!("payload serialization failed: {}", e)))?;
        let signature: Signature = self.signing_key.sign(&payload_bytes);
        Ok(SignedHeaders {
            api_key: self.api_key.clone(),
            timestamp: req.timestamp_ms.to_string(),
            sign_version: SIGN_VERSION.to_string(),
            idempotency_key,
            data: URL_SAFE_NO_PAD.encode(&payload_bytes),
            sign: URL_SAFE_NO_PAD.encode(signature.to_bytes()),
        })
    }
}

/// 被签名的内容。`path_and_query` 必须与实际上线的字节完全一致。
#[derive(Debug, Clone, Copy)]
pub struct SignRequest<'a> {
    /// 大写 HTTP 方法；WebSocket 握手按 `GET` 签名。
    pub method: &'a str,
    /// 例如 `/v1/trading/accounts/123/positions?volume=0.01`
    pub path_and_query: &'a str,
    /// 原始请求体（无 body 的 GET/DELETE 为空）。
    pub body: &'a [u8],
    /// GET 时为 `None`（请求头发送空值）。变更类调用必填。
    pub idempotency_key: Option<&'a str>,
    pub timestamp_ms: i64,
}

/// 六个 `EXN-*` 请求头的值，可直接附加到 HTTP 或 WebSocket 请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedHeaders {
    pub api_key: String,
    pub timestamp: String,
    pub sign_version: String,
    pub idempotency_key: String,
    pub data: String,
    pub sign: String,
}

impl SignedHeaders {
    /// 按稳定顺序排列的 `(name, value)` 对。
    pub fn iter(&self) -> impl Iterator<Item = (&'static str, &str)> {
        [
            (HEADER_API_KEY, self.api_key.as_str()),
            (HEADER_TIMESTAMP, self.timestamp.as_str()),
            (HEADER_SIGN_VERSION, self.sign_version.as_str()),
            (HEADER_IDEMPOTENCY_KEY, self.idempotency_key.as_str()),
            (HEADER_DATA, self.data.as_str()),
            (HEADER_SIGN, self.sign.as_str()),
        ]
        .into_iter()
    }

    /// 解码后的载荷字节（服务端据此验签）。
    pub fn payload_bytes(&self) -> Result<Vec<u8>, AuthError> {
        URL_SAFE_NO_PAD
            .decode(&self.data)
            .map_err(|e| AuthError::InvalidPem(format!("base64 decode failed: {}", e)))
    }
}

/// 字段顺序固定；serde 保留结构体顺序，因此字节可复现。
#[derive(Serialize)]
struct Payload<'a> {
    api_key: &'a str,
    idempotency_key: &'a str,
    timestamp: i64,
    sign_version: u8,
    method: &'a str,
    path: &'a str,
    body_hash: &'a str,
}

/// SHA-256(body) 的 base64url（无填充）。
pub fn body_hash(body: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(body))
}

/// 当前 Unix 毫秒时间。
pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// 校验 Exness 约束：长度 5..=128，字符集 `[A-Za-z0-9._~-]`。
pub fn validate_idempotency_key(key: &str) -> Result<(), AuthError> {
    let ok_len = (IDEMPOTENCY_KEY_MIN..=IDEMPOTENCY_KEY_MAX).contains(&key.len());
    let ok_chars = key.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'~' | b'-'));
    if ok_len && ok_chars {
        Ok(())
    } else {
        Err(AuthError::InvalidIdempotencyKey(key.to_owned()))
    }
}

/// 生成 `{prefix}.{unix_ms}.{rand6}` 并校验。`prefix` 应已完成
/// 允许的字符集（例如 `gl.123456.grid.0`）。
pub fn generate_idempotency_key(prefix: &str) -> Result<String, AuthError> {
    use rand::Rng;
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    let rand6: String = (0..6).map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char).collect();
    let key = format!("{prefix}.{}.{rand6}", now_ms());
    validate_idempotency_key(&key)?;
    Ok(key)
}

fn parse_private_key(input: &str) -> Result<SigningKey, AuthError> {
    let trimmed = input.trim();
    if trimmed.starts_with("-----BEGIN") {
        return SigningKey::from_pkcs8_pem(trimmed).map_err(|e| AuthError::InvalidPem(e.to_string()));
    }
    let hex_str = trimmed.strip_prefix("0x").or_else(|| trimmed.strip_prefix("0X")).unwrap_or(trimmed);
    match hex_str.len() {
        64 => {
            let bytes = Zeroizing::new(hex::decode(hex_str)?);
            let seed: [u8; SECRET_KEY_LENGTH] = bytes.as_slice()
                .try_into()
                .map_err(|_| AuthError::UnrecognizedPrivateKeyFormat)?;
            Ok(SigningKey::from_bytes(&seed))
        }
        128 => {
            let bytes = Zeroizing::new(hex::decode(hex_str)?);
            if bytes.len() < SECRET_KEY_LENGTH * 2 {
                return Err(AuthError::UnrecognizedPrivateKeyFormat);
            }
            let seed: [u8; SECRET_KEY_LENGTH] = bytes[..SECRET_KEY_LENGTH]
                .try_into()
                .map_err(|_| AuthError::UnrecognizedPrivateKeyFormat)?;
            let key = SigningKey::from_bytes(&seed);
            if key.verifying_key().as_bytes() != &bytes[SECRET_KEY_LENGTH..] {
                return Err(AuthError::PublicKeyMismatch);
            }
            Ok(key)
        }
        _ => Err(AuthError::UnrecognizedPrivateKeyFormat),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::pkcs8::EncodePrivateKey;
    use ed25519_dalek::Verifier;

    fn test_key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    fn exness_style_hex(key: &SigningKey) -> String {
        format!("{}{}", hex::encode(key.to_bytes()), hex::encode(key.verifying_key().as_bytes()))
    }

    #[test]
    fn empty_body_hash_matches_spec_constant() {
        assert_eq!(body_hash(b""), EMPTY_BODY_HASH);
    }

    #[test]
    fn parses_128_hex_seed_and_public_key() {
        let key = test_key();
        let creds = Credentials::new("exnsk-test", &exness_style_hex(&key)).unwrap();
        assert_eq!(creds.verifying_key(), key.verifying_key());
    }

    #[test]
    fn rejects_128_hex_with_wrong_public_half() {
        let key = test_key();
        let mut s = exness_style_hex(&key);
        s.replace_range(126..128, "00");
        let err = Credentials::new("exnsk-test", &s).unwrap_err();
        assert!(matches!(err, AuthError::PublicKeyMismatch));
    }

    #[test]
    fn parses_64_hex_seed_and_0x_prefix() {
        let key = test_key();
        let seed_hex = hex::encode(key.to_bytes());
        let a = Credentials::new("k", &seed_hex).unwrap();
        let b = Credentials::new("k", &format!("0x{seed_hex}")).unwrap();
        assert_eq!(a.verifying_key(), key.verifying_key());
        assert_eq!(b.verifying_key(), key.verifying_key());
    }

    #[test]
    fn parses_pkcs8_pem() {
        let key = test_key();
        let pem = key.to_pkcs8_pem(ed25519_dalek::pkcs8::spki::der::pem::LineEnding::LF).unwrap();
        let creds = Credentials::new("k", &pem).unwrap();
        assert_eq!(creds.verifying_key(), key.verifying_key());
    }

    #[test]
    fn rejects_garbage() {
        assert!(matches!(Credentials::new("k", "not a key").unwrap_err(), AuthError::UnrecognizedPrivateKeyFormat));
        assert!(matches!(Credentials::new("", &exness_style_hex(&test_key())).unwrap_err(), AuthError::EmptyApiKey));
    }

    #[test]
    fn get_request_has_empty_idempotency_key_and_verifiable_signature() {
        let creds = Credentials::new("exnsk-abc", &exness_style_hex(&test_key())).unwrap();
        let headers = creds
            .sign(&SignRequest {
                method: "GET",
                path_and_query: "/v1/trading/access-point?account_id=123",
                body: b"",
                idempotency_key: None,
                timestamp_ms: 1_773_656_070_123,
            })
            .unwrap();

        assert_eq!(headers.api_key, "exnsk-abc");
        assert_eq!(headers.timestamp, "1773656070123");
        assert_eq!(headers.sign_version, "1");
        assert_eq!(headers.idempotency_key, "");

        let payload = headers.payload_bytes().unwrap();
        assert_eq!(
            String::from_utf8(payload.clone()).unwrap(),
            r#"{"api_key":"exnsk-abc","idempotency_key":"","timestamp":1773656070123,"sign_version":1,"method":"GET","path":"/v1/trading/access-point?account_id=123","body_hash":"47DEQpj8HBSa-_TImW-5JCeuQeRkm5NMpJWZG3hSuFU"}"#
        );
        let sig_bytes = URL_SAFE_NO_PAD.decode(&headers.sign).unwrap();
        let sig = Signature::from_slice(&sig_bytes).unwrap();
        creds.verifying_key().verify(&payload, &sig).unwrap();

        // 六个请求头均存在，且顺序正确
        let names: Vec<&str> = headers.iter().map(|(n, _)| n).collect();
        assert_eq!(
            names,
            [HEADER_API_KEY, HEADER_TIMESTAMP, HEADER_SIGN_VERSION, HEADER_IDEMPOTENCY_KEY, HEADER_DATA, HEADER_SIGN]
        );
    }

    #[test]
    fn post_request_hashes_body_and_carries_idempotency_key() {
        let creds = Credentials::new("exnsk-abc", &exness_style_hex(&test_key())).unwrap();
        let body = br#"{"instrument":"XAUUSD","side":"buy","volume":"0.01"}"#;
        let headers = creds
            .sign(&SignRequest {
                method: "POST",
                path_and_query: "/v1/trading/accounts/1/positions",
                body,
                idempotency_key: Some("gl.1.grid.0.1773656070123.abc123"),
                timestamp_ms: 1,
            })
            .unwrap();
        assert_eq!(headers.idempotency_key, "gl.1.grid.0.1773656070123.abc123");
        let payload: serde_json::Value = serde_json::from_slice(&headers.payload_bytes().unwrap()).unwrap();
        assert_eq!(payload["body_hash"], body_hash(body));
        assert_ne!(payload["body_hash"], EMPTY_BODY_HASH);
        assert_eq!(payload["method"], "POST");
    }

    #[test]
    fn idempotency_key_rules() {
        assert!(validate_idempotency_key("abcde").is_ok());
        assert!(validate_idempotency_key("gl.1.grid.0.1773656070123.abc123").is_ok());
        assert!(validate_idempotency_key("A-Z_a~z.09").is_ok());
        assert!(validate_idempotency_key("abcd").is_err()); // 过短
        assert!(validate_idempotency_key(&"a".repeat(129)).is_err()); // 过长
        assert!(validate_idempotency_key("has space").is_err());
        assert!(validate_idempotency_key("has/slash").is_err());
        assert!(validate_idempotency_key("has+plus").is_err());

        let k = generate_idempotency_key("gl.123456.grid.0").unwrap();
        assert!(k.starts_with("gl.123456.grid.0."));
        assert_eq!(k.split('.').count(), 6);
    }

    #[test]
    fn spki_public_key_is_44_char_base64() {
        let creds = Credentials::new("k", &exness_style_hex(&test_key())).unwrap();
        let spki = creds.public_key_spki_base64().unwrap();
        // 12 字节 Ed25519 SPKI 前缀 + 32 字节密钥 = 44 字节 -> 60 个 base64 字符
        assert_eq!(spki.len(), 60);
        assert!(spki.starts_with("MCowBQYDK2VwAyEA"));
    }
}
