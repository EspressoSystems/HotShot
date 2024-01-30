use thiserror::Error;

#[derive(Debug, Error)]
#[error("{0}")]
pub enum BrokerError {
    FileError(String),
    ReadError(String),
    WriteError(String),
    ParseError(String),
    ConnectionError(String),
    StreamError(String),
    BindError(String),
    SerializeError(String),
    DeserializeError(String),
    CryptoError(String),
    AuthenticationError(String),
    RedisError(String),
}

pub type Result<T> = std::result::Result<T, BrokerError>;

#[macro_export]
macro_rules! err {
    ($expr: expr, $type: ident, $context: expr) => {
        $expr.map_err(|err| BrokerError::$type(format!("{}: {err}", $context)))
    };
}

#[macro_export]
macro_rules! op {
    ($expr: expr, $type: ident, $context: expr) => {
        $expr.ok_or_else(|| BrokerError::$type($context.to_string()))
    };
}
