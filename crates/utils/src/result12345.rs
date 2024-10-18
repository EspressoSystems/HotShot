use std::{cmp::max, fmt::Display};

/// Macros
mod macros;
#[allow(unused_imports)]
pub use macros::*;

/// Default log level for the crate
pub const DEFAULT_LOG_LEVEL: Level = Level::Info;

/// Trait for logging errors
pub trait Log {
    /// Log an error via `tracing` utilities, printing it.
    fn log(&self);
}

impl Log for Error {
    fn log(&self) {
        let mut error_level = self.level;
        if error_level == Level::Unspecified {
            error_level = DEFAULT_LOG_LEVEL;
        }

        match error_level {
            Level::Trace => {
                tracing::trace!("{}", self.message);
            }
            Level::Debug => {
                tracing::debug!("{}", self.message);
            }
            Level::Info => {
                tracing::info!("{}", self.message);
            }
            Level::Warn => {
                tracing::warn!("{}", self.message);
            }
            Level::Error => {
                tracing::error!("{}", self.message);
            }
            // impossible
            Level::Unspecified => {}
        }
    }
}

impl<T> Log for Result<T> {
    fn log(&self) {
        let error = match self {
            Ok(_) => {
                return;
            }
            Err(e) => e,
        };

        let mut error_level = error.level;
        if error_level == Level::Unspecified {
            error_level = DEFAULT_LOG_LEVEL;
        }

        match error_level {
            Level::Trace => {
                tracing::trace!("{}", error.message);
            }
            Level::Debug => {
                tracing::debug!("{}", error.message);
            }
            Level::Info => {
                tracing::info!("{}", error.message);
            }
            Level::Warn => {
                tracing::warn!("{}", error.message);
            }
            Level::Error => {
                tracing::error!("{}", error.message);
            }
            // impossible
            Level::Unspecified => {}
        }
    }
}

#[derive(Debug, Clone)]
#[must_use]
/// main error type
pub struct Error {
    /// level
    pub level: Level,
    /// message
    pub message: String,
}

impl std::error::Error for Error {}

/// Trait for a `std::result::Result` that can be wrapped into a `Result`
pub trait Wrap<T> {
    /// Wrap the value into a `Result`
    ///
    /// # Errors
    /// Propagates errors from `self`
    fn wrap(self) -> Result<T>;
}

impl<T, E> Wrap<T> for std::result::Result<T, E>
where
    E: Display,
{
    fn wrap(self) -> Result<T> {
        match self {
            Ok(t) => Ok(t),
            Err(e) => Err(Error {
                level: Level::Unspecified,
                message: format!("{e}"),
            }),
        }
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

/// Alias for the main `Result` type used by the crate.
pub type Result<T> = std::result::Result<T, Error>;

#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Clone, Copy)]
/// Possible log levels
pub enum Level {
    /// Unspecified log level
    Unspecified,
    /// TRACE
    Trace,
    /// DEBUG
    Debug,
    /// INFO
    Info,
    /// WARN
    Warn,
    /// ERROR
    Error,
}

/// Prepend an error to its cause
fn concatenate(error: &String, cause: &String) -> String {
    format!("{error}\ncaused by: {cause}")
}

/// Trait for converting error types to a `Result<T>`.
pub trait Context<T> {
    /// Attach context to the given error.
    ///
    /// # Errors
    /// Propagates errors from `self`
    fn context(self, error: Error) -> Result<T>;
}

impl<T> Context<T> for Result<T> {
    fn context(self, error: Error) -> Result<T> {
        match self {
            Ok(t) => Ok(t),
            Err(cause) => Err(Error {
                level: max(error.level, cause.level),
                message: concatenate(&error.message, &format!("{cause}")),
            }),
        }
    }
}

impl<T> Context<T> for Option<T> {
    fn context(self, error: Error) -> Result<T> {
        match self {
            Some(t) => Ok(t),
            None => Err(error),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn ordering() {
        assert!(Level::Trace < Level::Debug);
        assert!(Level::Debug < Level::Info);
        assert!(Level::Info < Level::Warn);
        assert!(Level::Warn < Level::Error);
        assert!(max(Level::Trace, Level::Error) == Level::Error);
    }
}
