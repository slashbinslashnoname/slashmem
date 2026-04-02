use std::io::IsTerminal;

/// Determines how command output should be formatted.
///
/// When stdout is a TTY (interactive terminal), output defaults to a
/// human-readable format.  When piped (non-TTY), output defaults to JSON
/// so downstream tools can parse it reliably.
///
/// The `--json` flag forces JSON regardless of TTY status.
/// The `--quiet` flag suppresses all normal stdout output.
#[derive(Debug, Clone, PartialEq)]
pub struct FormatContext {
    pub is_tty: bool,
    pub json: bool,
    pub quiet: bool,
}

impl FormatContext {
    /// Auto-detect from the environment and CLI flags.
    ///
    /// Rules:
    /// - `is_tty` is true when stdout is a terminal.
    /// - If `json_flag` is set, JSON output is forced.
    /// - If stdout is not a TTY, JSON output is the default.
    /// - `quiet` suppresses normal output entirely.
    pub fn detect(json_flag: bool, quiet: bool) -> Self {
        let is_tty = std::io::stdout().is_terminal();
        Self {
            is_tty,
            json: json_flag || !is_tty,
            quiet,
        }
    }

    /// Build from explicit values (useful in tests).
    pub fn new(is_tty: bool, json: bool, quiet: bool) -> Self {
        Self { is_tty, json, quiet }
    }

    /// Whether output should be JSON-formatted.
    pub fn use_json(&self) -> bool {
        self.json
    }

    /// Whether normal output should be suppressed.
    pub fn is_quiet(&self) -> bool {
        self.quiet
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_tty_defaults_to_json() {
        // detect() sets json=true when not a TTY and no explicit json flag.
        // The test harness always runs with captured (non-TTY) stdout.
        let ctx = FormatContext::detect(false, false);
        assert!(!ctx.is_tty);
        assert!(ctx.use_json());
    }

    #[test]
    fn tty_defaults_to_non_json() {
        let ctx = FormatContext::new(true, false, false);
        assert!(!ctx.use_json());
    }

    #[test]
    fn json_flag_forces_json_even_on_tty() {
        let ctx = FormatContext::new(true, true, false);
        assert!(ctx.use_json());
    }

    #[test]
    fn quiet_flag() {
        let ctx = FormatContext::new(true, false, true);
        assert!(ctx.is_quiet());
        assert!(!ctx.use_json());
    }

    #[test]
    fn quiet_and_json_together() {
        let ctx = FormatContext::new(false, true, true);
        assert!(ctx.is_quiet());
        assert!(ctx.use_json());
    }

    #[test]
    fn detect_in_test_harness() {
        // In test, stdout is captured (not a TTY), so detect should
        // default to JSON output.
        let ctx = FormatContext::detect(false, false);
        assert!(!ctx.is_tty);
        assert!(ctx.use_json());
        assert!(!ctx.is_quiet());
    }

    #[test]
    fn detect_with_json_flag() {
        let ctx = FormatContext::detect(true, false);
        assert!(ctx.use_json());
    }

    #[test]
    fn detect_with_quiet_flag() {
        let ctx = FormatContext::detect(false, true);
        assert!(ctx.is_quiet());
    }

    #[test]
    fn new_preserves_all_fields() {
        let ctx = FormatContext::new(true, true, true);
        assert!(ctx.is_tty);
        assert!(ctx.json);
        assert!(ctx.quiet);

        let ctx = FormatContext::new(false, false, false);
        assert!(!ctx.is_tty);
        assert!(!ctx.json);
        assert!(!ctx.quiet);
    }

    #[test]
    fn clone_and_eq() {
        let a = FormatContext::new(true, false, true);
        let b = a.clone();
        assert_eq!(a, b);
    }
}
