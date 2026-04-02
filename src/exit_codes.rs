/// Semantic exit codes for the `sm` CLI.
///
/// These codes let callers (scripts, agents, CI) distinguish error categories
/// without parsing stderr text.

/// Command completed successfully.
pub const SUCCESS: i32 = 0;

/// Unspecified / general error.
pub const GENERAL_ERROR: i32 = 1;

/// Invalid arguments or usage error.
pub const INVALID_ARGS: i32 = 2;

/// Requested resource was not found.
pub const NOT_FOUND: i32 = 3;

/// I/O error (file system, network, etc.).
pub const IO_ERROR: i32 = 4;

/// Configuration or environment error.
pub const CONFIG_ERROR: i32 = 5;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_distinct() {
        let codes = [SUCCESS, GENERAL_ERROR, INVALID_ARGS, NOT_FOUND, IO_ERROR, CONFIG_ERROR];
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
        for code in [SUCCESS, GENERAL_ERROR, INVALID_ARGS, NOT_FOUND, IO_ERROR, CONFIG_ERROR] {
            assert!(code >= 0, "exit code {code} must be non-negative");
        }
    }

    #[test]
    fn success_is_zero() {
        assert_eq!(SUCCESS, 0);
    }
}
