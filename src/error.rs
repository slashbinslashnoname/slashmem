use std::fmt;

/// Unified error type for slashmem.
#[derive(Debug)]
pub enum AppError {
    /// Database errors from rusqlite.
    Database(rusqlite::Error),
    /// JSON serialization/deserialization errors.
    Json(serde_json::Error),
    /// I/O errors.
    Io(std::io::Error),
    /// General-purpose error with a message.
    Other(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Database(e) => write!(f, "database error: {e}"),
            AppError::Json(e) => write!(f, "json error: {e}"),
            AppError::Io(e) => write!(f, "io error: {e}"),
            AppError::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for AppError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AppError::Database(e) => Some(e),
            AppError::Json(e) => Some(e),
            AppError::Io(e) => Some(e),
            AppError::Other(_) => None,
        }
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        AppError::Database(e)
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::Json(e)
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Io(e)
    }
}

/// Unified Result alias for slashmem.
pub type Result<T> = std::result::Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_error_from_rusqlite() {
        let sqlite_err = rusqlite::Error::InvalidColumnName("test".into());
        let err: AppError = sqlite_err.into();
        assert!(matches!(err, AppError::Database(_)));
        assert!(err.to_string().contains("database error"));
    }

    #[test]
    fn app_error_from_serde_json() {
        let json_err = serde_json::from_str::<String>("not json").unwrap_err();
        let err: AppError = json_err.into();
        assert!(matches!(err, AppError::Json(_)));
        assert!(err.to_string().contains("json error"));
    }

    #[test]
    fn app_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "gone");
        let err: AppError = io_err.into();
        assert!(matches!(err, AppError::Io(_)));
        assert!(err.to_string().contains("io error"));
    }

    #[test]
    fn app_error_other() {
        let err = AppError::Other("something went wrong".into());
        assert_eq!(err.to_string(), "something went wrong");
    }

    #[test]
    fn app_error_source() {
        use std::error::Error;
        let err = AppError::Other("no source".into());
        assert!(err.source().is_none());

        let db_err: AppError = rusqlite::Error::InvalidColumnName("x".into()).into();
        assert!(db_err.source().is_some());
    }

    #[test]
    fn result_alias_works() {
        fn ok_fn() -> Result<i32> {
            Ok(42)
        }
        fn err_fn() -> Result<i32> {
            Err(AppError::Other("fail".into()))
        }
        assert_eq!(ok_fn().unwrap(), 42);
        assert!(err_fn().is_err());
    }
}
