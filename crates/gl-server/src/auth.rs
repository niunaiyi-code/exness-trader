//! JWT 认证与密码哈希（docs/06 §2）。

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AuthError {
    #[error("Password hashing failed: {0}")]
    HashError(String),
    #[error("Invalid password")]
    InvalidPassword,
    #[error("JWT error: {0}")]
    JwtError(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,    // user_id
    pub role: String,   // 管理员/交易员/观察者
    pub exp: u64,       // 过期时间戳
}

/// 使用 Argon2id 对密码做哈希。
pub fn hash_password(password: &str) -> Result<String, AuthError> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| AuthError::HashError(e.to_string()))?;
    Ok(hash.to_string())
}

/// 用 Argon2 哈希校验密码。
pub fn verify_password(password: &str, hash: &str) -> Result<(), AuthError> {
    let parsed_hash = PasswordHash::new(hash)
        .map_err(|e| AuthError::HashError(e.to_string()))?;
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .map_err(|_| AuthError::InvalidPassword)
}

/// 创建 JWT token。
pub fn create_jwt(user_id: &str, role: &str, secret: &str, expiry_hours: u64) -> Result<String, AuthError> {
    let expiration = chrono::Utc::now()
        .checked_add_signed(chrono::Duration::hours(expiry_hours as i64))
        .ok_or_else(|| AuthError::JwtError("Invalid expiry time".to_string()))?
        .timestamp() as u64;

    let claims = Claims {
        sub: user_id.to_string(),
        role: role.to_string(),
        exp: expiration,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| AuthError::JwtError(e.to_string()))
}

/// 校验并解码 JWT token。
pub fn verify_jwt(token: &str, secret: &str) -> Result<Claims, AuthError> {
    let validation = Validation::default();
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map_err(|e| AuthError::JwtError(e.to_string()))?;

    Ok(token_data.claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_hash_verify() {
        let password = "secure_password_123";
        let hash = hash_password(password).unwrap();
        assert!(verify_password(password, &hash).is_ok());
        assert!(verify_password("wrong_password", &hash).is_err());
    }

    #[test]
    fn jwt_create_verify() {
        let secret = "test_jwt_secret";
        let token = create_jwt("user_123", "admin", secret, 24).unwrap();
        let claims = verify_jwt(&token, secret).unwrap();
        assert_eq!(claims.sub, "user_123");
        assert_eq!(claims.role, "admin");
    }

    #[test]
    fn jwt_invalid_signature() {
        let token = create_jwt("user_123", "admin", "secret1", 24).unwrap();
        let result = verify_jwt(&token, "secret2");
        assert!(result.is_err());
    }
}
