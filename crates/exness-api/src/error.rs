//! 错误类型及调用方使用的重试分类（见 docs/04 §9）。

use crate::auth::AuthError;

/// 粗粒度分类，决定调用方下一步动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorClass {
    /// 凭据 / 签名 / 账户问题：停止运行器、告警，绝不盲目重试。
    Fatal,
    /// 网络错误、5xx、429：可安全按退避重试。
    Transient,
    /// 请求已被理解但被交易规则/市场状态拒绝：由策略决定。
    Business,
    /// 我们发出了格式错误的内容（4xx REQUEST_*）：属编程错误，记录并上报。
    Client,
}

/// Exness 公开错误码区间（OpenAPI 1.0.2）。
pub mod code {
    pub const SYSTEM_MIN: i32 = 1;
    pub const SYSTEM_MAX: i32 = 3;
    pub const AUTH_MIN: i32 = 1000;
    pub const AUTH_MAX: i32 = 1002;
    pub const ACCOUNT_MIN: i32 = 2000;
    pub const ACCOUNT_MAX: i32 = 2002;
    pub const ACCOUNT_CLOSE_ONLY: i32 = 2002;
    pub const REQUEST_MIN: i32 = 3000;
    pub const REQUEST_MAX: i32 = 3015;
    pub const REQUEST_RATE_LIMIT: i32 = 3015;
    pub const MARKET_MIN: i32 = 4000;
    pub const MARKET_MAX: i32 = 4004;
    pub const TRADING_RULE_MIN: i32 = 5000;
    pub const TRADING_RULE_MAX: i32 = 5010;
    pub const EXECUTION_MIN: i32 = 6000;
    pub const EXECUTION_MAX: i32 = 6001;

    pub fn classify(code: i32) -> super::ErrorClass {
        use super::ErrorClass::*;
        match code {
            SYSTEM_MIN..=SYSTEM_MAX => Transient,
            AUTH_MIN..=AUTH_MAX => Fatal,
            ACCOUNT_MIN..=ACCOUNT_MAX => Fatal,
            REQUEST_RATE_LIMIT => Transient,
            REQUEST_MIN..=REQUEST_MAX => Client,
            MARKET_MIN..=MARKET_MAX => Business,
            TRADING_RULE_MIN..=TRADING_RULE_MAX => Business,
            EXECUTION_MIN..=EXECUTION_MAX => Business,
            _ => Client,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("credentials: {0}")]
    Auth(#[from] AuthError),

    #[error("transport: {0}")]
    Transport(#[from] reqwest::Error),

    #[error("invalid url: {0}")]
    Url(#[from] url::ParseError),

    /// 非 2xx HTTP 响应，可能带结构化的 Exness 错误体。
    #[error("api {status}: code={code:?} {message}")]
    Api {
        status: u16,
        code: Option<i32>,
        message: String,
        /// 服务端返回 `Retry-After` 时的毫秒值（429 / 503）。
        retry_after_ms: Option<u64>,
        /// 用于诊断的原始请求体；绝不包含密钥。
        raw: String,
    },

    #[error("decode {context}: {source}")]
    Decode {
        context: &'static str,
        #[source]
        source: serde_json::Error,
        raw: String,
    },

    #[error("websocket: {0}")]
    Ws(String),

    #[error("access point not discovered yet")]
    NoAccessPoint,

    /// 本 crate 内部的编程/配置错误（绝不会由服务端引起）。
    #[error("internal: {0}")]
    Internal(String),
}

impl Error {
    pub fn class(&self) -> ErrorClass {
        match self {
            Error::Auth(_) => ErrorClass::Fatal,
            Error::Transport(e) => {
                if e.is_builder() || e.is_request() && !e.is_timeout() && !e.is_connect() {
                    ErrorClass::Client
                } else {
                    ErrorClass::Transient
                }
            }
            Error::Url(_) => ErrorClass::Client,
            Error::Api { status, code, .. } => match (code, *status) {
                (Some(c), _) => code::classify(*c),
                (None, 401 | 403) => ErrorClass::Fatal,
                (None, 429 | 500..=599) => ErrorClass::Transient,
                (None, _) => ErrorClass::Client,
            },
            Error::Decode { .. } => ErrorClass::Client,
            Error::Ws(_) => ErrorClass::Transient,
            Error::NoAccessPoint => ErrorClass::Client,
            Error::Internal(_) => ErrorClass::Client,
        }
    }

    pub fn is_rate_limited(&self) -> bool {
        matches!(self, Error::Api { status: 429, .. } | Error::Api { code: Some(code::REQUEST_RATE_LIMIT), .. })
    }

    pub fn api_code(&self) -> Option<i32> {
        match self {
            Error::Api { code, .. } => *code,
            _ => None,
        }
    }

    pub(crate) fn decode(context: &'static str, raw: &[u8], source: serde_json::Error) -> Self {
        Error::Decode { context, source, raw: String::from_utf8_lossy(raw).into_owned() }
    }
}

impl From<tokio_tungstenite::tungstenite::Error> for Error {
    fn from(e: tokio_tungstenite::tungstenite::Error) -> Self {
        Error::Ws(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification_by_code_range() {
        assert_eq!(code::classify(1), ErrorClass::Transient);
        assert_eq!(code::classify(1001), ErrorClass::Fatal);
        assert_eq!(code::classify(2000), ErrorClass::Fatal);
        assert_eq!(code::classify(3001), ErrorClass::Client);
        assert_eq!(code::classify(3015), ErrorClass::Transient);
        assert_eq!(code::classify(4002), ErrorClass::Business);
        assert_eq!(code::classify(5003), ErrorClass::Business);
        assert_eq!(code::classify(6000), ErrorClass::Business);
        assert_eq!(code::classify(9999), ErrorClass::Client);
    }

    #[test]
    fn http_status_fallback_when_no_code() {
        let e = |status: u16| Error::Api {
            status,
            code: None,
            message: String::new(),
            retry_after_ms: None,
            raw: String::new(),
        };
        assert_eq!(e(401).class(), ErrorClass::Fatal);
        assert_eq!(e(429).class(), ErrorClass::Transient);
        assert!(e(429).is_rate_limited());
        assert_eq!(e(503).class(), ErrorClass::Transient);
        assert_eq!(e(400).class(), ErrorClass::Client);
    }
}
