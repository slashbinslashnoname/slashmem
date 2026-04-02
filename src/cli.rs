use clap::{Parser, Subcommand};

/// slashmem — a local memory store for AI agents
#[derive(Parser, Debug)]
#[command(name = "sm", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Query memory for relevant context given a task description
    Context(ContextArgs),
    /// Ingest an episodic record and process rule markers
    Ingest(IngestArgs),
    /// Run confidence decay, maturity transitions, and pruning
    Distill,
}

/// Arguments for the `context` subcommand.
#[derive(clap::Args, Debug)]
pub struct ContextArgs {
    /// Task description to search for relevant memories
    pub description: String,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

/// Arguments for the `ingest` subcommand.
#[derive(clap::Args, Debug)]
pub struct IngestArgs {
    /// Task identifier
    #[arg(long)]
    pub task: String,

    /// Body text (may contain rule markers)
    #[arg(long)]
    pub body: String,

    /// Agent identifier
    #[arg(long)]
    pub agent: String,

    /// Procedural rule IDs whose confidence should increase (repeatable)
    #[arg(long)]
    pub success: Vec<String>,

    /// Procedural rule IDs whose confidence should decrease (repeatable)
    #[arg(long)]
    pub harm: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_debug_assert() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parse_context_command() {
        let cli = Cli::parse_from(["sm", "context", "fix the login bug"]);
        match cli.command {
            Commands::Context(args) => {
                assert_eq!(args.description, "fix the login bug");
                assert!(!args.json);
            }
            _ => panic!("expected Context command"),
        }
    }

    #[test]
    fn parse_context_with_json() {
        let cli = Cli::parse_from(["sm", "context", "--json", "fix the login bug"]);
        match cli.command {
            Commands::Context(args) => {
                assert!(args.json);
            }
            _ => panic!("expected Context command"),
        }
    }

    #[test]
    fn parse_ingest_command() {
        let cli = Cli::parse_from([
            "sm", "ingest", "--task", "TASK-1", "--body", "did stuff", "--agent", "agent-0",
        ]);
        match cli.command {
            Commands::Ingest(args) => {
                assert_eq!(args.task, "TASK-1");
                assert_eq!(args.body, "did stuff");
                assert_eq!(args.agent, "agent-0");
                assert!(args.success.is_empty());
                assert!(args.harm.is_empty());
            }
            _ => panic!("expected Ingest command"),
        }
    }

    #[test]
    fn parse_ingest_with_success_flags() {
        let cli = Cli::parse_from([
            "sm", "ingest", "--task", "T-1", "--body", "ok", "--agent", "a",
            "--success", "rule-1", "--success", "rule-2",
        ]);
        match cli.command {
            Commands::Ingest(args) => {
                assert_eq!(args.success, vec!["rule-1", "rule-2"]);
                assert!(args.harm.is_empty());
            }
            _ => panic!("expected Ingest command"),
        }
    }

    #[test]
    fn parse_ingest_with_harm_flags() {
        let cli = Cli::parse_from([
            "sm", "ingest", "--task", "T-1", "--body", "bad", "--agent", "a",
            "--harm", "rule-3",
        ]);
        match cli.command {
            Commands::Ingest(args) => {
                assert!(args.success.is_empty());
                assert_eq!(args.harm, vec!["rule-3"]);
            }
            _ => panic!("expected Ingest command"),
        }
    }

    #[test]
    fn parse_ingest_with_both_success_and_harm() {
        let cli = Cli::parse_from([
            "sm", "ingest", "--task", "T-1", "--body", "mixed", "--agent", "a",
            "--success", "rule-1", "--harm", "rule-2", "--success", "rule-3",
        ]);
        match cli.command {
            Commands::Ingest(args) => {
                assert_eq!(args.success, vec!["rule-1", "rule-3"]);
                assert_eq!(args.harm, vec!["rule-2"]);
            }
            _ => panic!("expected Ingest command"),
        }
    }

    #[test]
    fn parse_distill_command() {
        let cli = Cli::parse_from(["sm", "distill"]);
        assert!(matches!(cli.command, Commands::Distill));
    }
}
