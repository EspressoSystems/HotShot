#[macro_export]
/// Print the file and line number of the location this macro is invoked
macro_rules! line_info {
    () => {
        format!("{}:{}", file!(), line!())
    };
}
pub use line_info;

#[macro_export]
/// Create an error at the trace level.
///
/// The argument can be either:
///   - an expression implementing `Display`
///   - a string literal
///   - a format string, similar to the `format!()` macro
macro_rules! trace {
  ($error:expr) => {
      Error {
        level: Level::Trace,
        message: format!("{}: {}", line_info!(), $error)
      }
  };
  ($message:literal) => {
      Error {
        level: Level::Trace,
        message: format!("{}: {}", line_info!(), $message)
      }
  };
  ($fmt:expr, $($arg:tt)*) => {
      Error {
        level: Level::Trace,
        message: format!("{}: {}", line_info!(), format!($fmt, $($arg)*))
      }
  };
}
pub use trace;

#[macro_export]
/// Create an error at the debug level.
///
/// The argument can be either:
///   - an expression implementing `Display`
///   - a string literal
///   - a format string, similar to the `format!()` macro
macro_rules! debug {
  ($error:expr) => {
      Error {
        level: Level::Debug,
        message: format!("{}: {}", line_info!(), $error)
      }
  };
  ($message:literal) => {
      Error {
        level: Level::Debug,
        message: format!("{}: {}", line_info!(), $message)
      }
  };
  ($fmt:expr, $($arg:tt)*) => {
      Error {
        level: Level::Debug,
        message: format!("{}: {}", line_info!(), format!($fmt, $($arg)*))
      }
  };
}
pub use debug;

#[macro_export]
/// Create an error at the info level.
///
/// The argument can be either:
///   - an expression implementing `Display`
///   - a string literal
///   - a format string, similar to the `format!()` macro
macro_rules! info {
  ($error:expr) => {
      Error {
        level: Level::Info,
        message: format!("{}: {}", line_info!(), $error)
      }
  };
  ($message:literal) => {
      Error {
        level: Level::Info,
        message: format!("{}: {}", line_info!(), $message)
      }
  };
  ($fmt:expr, $($arg:tt)*) => {
      Error {
        level: Level::Info,
        message: format!("{}: {}", line_info!(), format!($fmt, $($arg)*))
      }
  };
}
pub use info;

#[macro_export]
/// Create an error at the warn level.
///
/// The argument can be either:
///   - an expression implementing `Display`
///   - a string literal
///   - a format string, similar to the `format!()` macro
macro_rules! warn {
  ($error:expr) => {
      Error {
        level: Level::Warn,
        message: format!("{}: {}", line_info!(), $error)
      }
  };
  ($message:literal) => {
      Error {
        level: Level::Warn,
        message: format!("{}: {}", line_info!(), $message)
      }
  };
  ($fmt:expr, $($arg:tt)*) => {
      Error {
        level: Level::Warn,
        message: format!("{}: {}", line_info!(), format!($fmt, $($arg)*))
      }
  };
}
pub use crate::warn;

#[macro_export]
/// Create an error at the error level.
///
/// The argument can be either:
///   - an expression implementing `Display`
///   - a string literal
///   - a format string, similar to the `format!()` macro
macro_rules! error {
  ($error:expr) => {
      Error {
        level: Level::Error,
        message: format!("{}: {}", line_info!(), $error)
      }
  };
  ($message:literal) => {
      Error {
        level: Level::Error,
        message: format!("{}: {}", line_info!(), $message)
      }
  };
  ($fmt:expr, $($arg:tt)*) => {
      Error {
        level: Level::Error,
        message: format!("{}: {}", line_info!(), format!($fmt, $($arg)*))
      }
  };
}
pub use error;

#[macro_export]
/// Check that the given condition holds, otherwise return an error.
///
/// The argument can be either:
///   - a condition, in which case a generic error is logged at the `Unspecified` level.
///   - a condition and a string literal, in which case the provided literal is logged at the `Unspecified` level.
///   - a condition and a format expression, in which case the message is formatted and logged at the `Unspecified` level.
///   - a condition and an `Error`, in which case the given error is logged unchanged.
macro_rules! ensure {
  ($condition:expr) => {
      if !$condition {
        return Err(Error {
          level: Level::Unspecified,
          message: format!("{}: condition '{}' failed.", line_info!(), stringify!($condition))
        });
     }
  };
  ($condition:expr, $message:literal) => {
      if !$condition {
        return Err(Error {
          level: Level::Unspecified,
          message: format!("{}: {}", line_info!(), $message)
        });
      }
  };
  ($condition:expr, $fmt:expr, $($arg:tt)*) => {
      if !$condition {
        return Err(Error {
          level: Level::Unspecified,
          message: format!("{}: {}", line_info!(), format!($fmt, $($arg)*))
        });
      }
  };
  ($condition:expr, $error:expr) => {
      if !$condition {
        return Err($error);
      }
  };
}
pub use ensure;

#[macro_export]
/// Return an error.
///
/// The argument can be either:
///   - nothing, in which case a generic message is logged at the `Unspecified` level.
///   - a string literal, in which case the provided literal is logged at the `Unspecified` level.
///   - a format expression, in which case the message is formatted and logged at the `Unspecified` level.
///   - an `Error`, in which case the given error is logged unchanged.
macro_rules! bail {
  () => {
      return Err(Error {
        level: Level::Unspecified,
        message: format!("{}: bailed.", line_info!()),
      });
  };
  ($message:literal) => {
      return Err(Error {
        level: Level::Unspecified,
        message: format!("{}: {}", line_info!(), $message)
      });
  };
  ($fmt:expr, $($arg:tt)*) => {
      return Err(Error {
        level: Level::Unspecified,
        message: format!("{}: {}", line_info!(), format!($fmt, $($arg)*))
      });
  };
  ($error:expr) => {
      return Err($error);
  };
}
pub use bail;
