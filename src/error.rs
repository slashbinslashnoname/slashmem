use std::fmt;

use crate::exit_codes;

/// Unified error type for slashmem.
#[derive(Debug)]
pub enum AppError {
    /// A requested resource was not found.
    NotFound(String),
    /// Invalid user input or arguments.
    InvalidInput(String),
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
            AppError::NotFound(msg) => write!(f, "not found: {msg}"),
            AppError::InvalidInput(msg) => write!(f, "invalid input: {msg}"),
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
            AppError::NotFound(_) => None,
            AppError::InvalidInput(_) => None,
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

impl AppError {
    /// Return the semantic exit code for this error.
    pub fn exit_code(&self) -> i32 {
        match self {
            AppError::NotFound(_) => exit_codes::NOT_FOUND,
            AppError::InvalidInput(_) => exit_codes::INVALID_INPUT,
            AppError::Database(_) => exit_codes::DB_ERROR,
            AppError::Json(_) => exit_codes::PARSE_ERROR,
            AppError::Io(_) => exit_codes::IO_ERROR,
            AppError::Other(_) => exit_codes::INTERNAL,
        }
    }

    /// Return a machine-readable error code string for structured output.
    pub fn error_code(&self) -> &'static str {
        match self {
            AppError::NotFound(_) => "NOT_FOUND",
            AppError::InvalidInput(_) => "INVALID_INPUT",
            AppError::Database(_) => "DB_ERROR",
            AppError::Json(_) => "PARSE_ERROR",
            AppError::Io(_) => "IO_ERROR",
            AppError::Other(_) => "INTERNAL",
        }
    }

    /// Return contextual next-step hints for AI agents and callers.
    pub fn suggestions(&self) -> Vec<&'static str> {
        match self {
            AppError::NotFound(_) => vec![
                "Check that the resource ID exists",
                "Run: sm rules list to see all known IDs",
            ],
            AppError::InvalidInput(_) => vec![
                "Check the command syntax with: sm --help",
                "Verify all required arguments are provided",
            ],
            AppError::Database(_) => vec![
                "Ensure the memory database is accessible",
                "Check disk space and file permissions",
            ],
            AppError::Json(_) => vec![
                "Verify the input is valid JSON",
                "Check for encoding issues or truncated data",
            ],
            AppError::Io(_) => vec![
                "Check file paths and permissions",
                "Ensure the target file or directory exists",
            ],
            AppError::Other(_) => vec![
                "An unexpected error occurred",
                "Check the error message for details",
            ],
        }
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
    fn app_error_not_found_display() {
        let err = AppError::NotFound("rule-42".into());
        assert_eq!(err.to_string(), "not found: rule-42");
    }

    #[test]
    fn app_error_invalid_input_display() {
        let err = AppError::InvalidInput("missing argument".into());
        assert_eq!(err.to_string(), "invalid input: missing argument");
    }

    #[test]
    fn app_error_source() {
        use std::error::Error;
        let err = AppError::Other("no source".into());
        assert!(err.source().is_none());

        let err = AppError::NotFound("x".into());
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

    // exit_code tests

    #[test]
    fn exit_code_not_found() {
        let err = AppError::NotFound("x".into());
        assert_eq!(err.exit_code(), exit_codes::NOT_FOUND);
        assert_eq!(err.exit_code(), 1);
    }

    #[test]
    fn exit_code_invalid_input() {
        let err = AppError::InvalidInput("bad arg".into());
        assert_eq!(err.exit_code(), exit_codes::INVALID_INPUT);
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn exit_code_database() {
        let err: AppError = rusqlite::Error::InvalidColumnName("x".into()).into();
        assert_eq!(err.exit_code(), exit_codes::DB_ERROR);
        assert_eq!(err.exit_code(), 3);
    }

    #[test]
    fn exit_code_json() {
        let err: AppError = serde_json::from_str::<String>("bad").unwrap_err().into();
        assert_eq!(err.exit_code(), exit_codes::PARSE_ERROR);
        assert_eq!(err.exit_code(), 5);
    }

    #[test]
    fn exit_code_io() {
        let err: AppError =
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied").into();
        assert_eq!(err.exit_code(), exit_codes::IO_ERROR);
        assert_eq!(err.exit_code(), 4);
    }

    #[test]
    fn exit_code_other() {
        let err = AppError::Other("something".into());
        assert_eq!(err.exit_code(), exit_codes::INTERNAL);
        assert_eq!(err.exit_code(), 127);
    }

    // error_code tests

    #[test]
    fn error_code_variants() {
        assert_eq!(AppError::NotFound("x".into()).error_code(), "NOT_FOUND");
        assert_eq!(AppError::InvalidInput("x".into()).error_code(), "INVALID_INPUT");
        assert_eq!(
            AppError::Database(rusqlite::Error::InvalidColumnName("x".into())).error_code(),
            "DB_ERROR"
        );
        assert_eq!(AppError::Io(std::io::Error::new(std::io::ErrorKind::Other, "x")).error_code(), "IO_ERROR");
        assert_eq!(AppError::Other("x".into()).error_code(), "INTERNAL");
    }

    #[test]
    fn error_code_parse_error() {
        let err: AppError = serde_json::from_str::<String>("bad").unwrap_err().into();
        assert_eq!(err.error_code(), "PARSE_ERROR");
    }

    // suggestions tests

    #[test]
    fn suggestions_not_found_non_empty() {
        let hints = AppError::NotFound("x".into()).suggestions();
        assert!(!hints.is_empty());
    }

    #[test]
    fn suggestions_invalid_input_non_empty() {
        let hints = AppError::InvalidInput("x".into()).suggestions();
        assert!(!hints.is_empty());
    }

    #[test]
    fn suggestions_all_variants_non_empty() {
        let errors: Vec<AppError> = vec![
            AppError::NotFound("x".into()),
            AppError::InvalidInput("x".into()),
            AppError::Database(rusqlite::Error::InvalidColumnName("x".into())),
            AppError::Json(serde_json::from_str::<String>("bad").unwrap_err()),
            AppError::Io(std::io::Error::new(std::io::ErrorKind::Other, "x")),
            AppError::Other("x".into()),
        ];
        for err in &errors {
            assert!(
                !err.suggestions().is_empty(),
                "suggestions() must not be empty for {:?}",
                err
            );
        }
    }
}
