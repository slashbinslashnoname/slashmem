/// Semantic exit codes for the `sm` CLI.
///
/// These codes let callers (scripts, agents, CI) distinguish error categories
/// without parsing stderr text.

/// Command completed successfully.
pub const SUCCESS: i32 = 0;

/// Requested resource was not found.
pub const NOT_FOUND: i32 = 1;

/// Invalid arguments or usage error.
pub const INVALID_INPUT: i32 = 2;

/// Database error.
pub const DB_ERROR: i32 = 3;

/// I/O error (file system, network, etc.).
pub const IO_ERROR: i32 = 4;

/// JSON parse / serialization error.
pub const PARSE_ERROR: i32 = 5;

/// Internal / unclassified error.
pub const INTERNAL: i32 = 127;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_distinct() {
        let codes = [SUCCESS, NOT_FOUND, INVALID_INPUT, DB_ERROR, IO_ERROR, PARSE_ERROR, INTERNAL];
        for (i, a) in codes.iter().enumerate() {
            for (j, b) in codes.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b, "exit codes at index {i} and {j} must differ");
                }
            }
        }
    }

    #[test]
    fn codes_are_non_negative() {
        for code in [SUCCESS, NOT_FOUND, INVALID_INPUT, DB_ERROR, IO_ERROR, PARSE_ERROR, INTERNAL] {
            assert!(code >= 0, "exit code {code} must be non-negative");
        }
    }

    #[test]
    fn success_is_zero() {
        assert_eq!(SUCCESS, 0);
    }

    #[test]
    fn not_found_is_one() {
        assert_eq!(NOT_FOUND, 1);
    }

    #[test]
    fn internal_is_127() {
        assert_eq!(INTERNAL, 127);
    }
}
