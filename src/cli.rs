use clap::{Parser, Subcommand};

/// slashmem — a local memory store for AI agents
#[derive(Parser, Debug)]
#[command(name = "sm", version, about, arg_required_else_help = false)]
pub struct Cli {
    /// Force JSON output (default when stdout is not a TTY)
    #[arg(long, global = true)]
    pub json: bool,

    /// Suppress normal stdout output
    #[arg(long, global = true)]
    pub quiet: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Query memory for relevant context given a task description
    Context(ContextArgs),
    /// Ingest an episodic record and process rule markers
    Ingest(IngestArgs),
    /// Run confidence decay, maturity transitions, and pruning
    Distill,
    /// Show database health and record counts
    Status,
    /// Manage procedural rules
    Rules(RulesArgs),
    /// Display agent integration prompt (for CLAUDE.md / system prompts)
    Prompt,
}

/// Arguments for the `rules` subcommand.
#[derive(clap::Args, Debug)]
pub struct RulesArgs {
    #[command(subcommand)]
    pub action: Option<RulesAction>,
}

#[derive(Subcommand, Debug)]
pub enum RulesAction {
    /// List all procedural rules
    List(RulesListArgs),
    /// Add a new procedural rule
    Add(RulesAddArgs),
    /// Remove a procedural rule by ID
    Rm(RulesRmArgs),
    /// Show details of a single rule
    Show(RulesShowArgs),
}

/// Arguments for `rules list`.
#[derive(clap::Args, Debug)]
pub struct RulesListArgs {
    /// Filter rules by full-text search query
    #[arg(long)]
    pub query: Option<String>,
}

/// Arguments for `rules add`.
#[derive(clap::Args, Debug)]
pub struct RulesAddArgs {
    /// Unique identifier for the rule
    pub id: String,

    /// Rule text
    pub rule: String,

    /// Source of the rule (e.g. "code-review", "postmortem")
    #[arg(long)]
    pub source: Option<String>,
}

/// Arguments for `rules rm`.
#[derive(clap::Args, Debug)]
pub struct RulesRmArgs {
    /// ID of the rule to remove
    pub id: String,
}

/// Arguments for `rules show`.
#[derive(clap::Args, Debug)]
pub struct RulesShowArgs {
    /// ID of the rule to show
    pub id: String,
}

/// Arguments for the `context` subcommand.
#[derive(clap::Args, Debug)]
pub struct ContextArgs {
    /// Task description to search for relevant memories
    pub description: String,
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
        assert!(!cli.json);
        assert!(!cli.quiet);
        match cli.command {
            Some(Commands::Context(args)) => {
                assert_eq!(args.description, "fix the login bug");
            }
            _ => panic!("expected Context command"),
        }
    }

    #[test]
    fn parse_global_json_flag() {
        let cli = Cli::parse_from(["sm", "--json", "context", "fix the login bug"]);
        assert!(cli.json);
        assert!(!cli.quiet);
    }

    #[test]
    fn parse_global_quiet_flag() {
        let cli = Cli::parse_from(["sm", "--quiet", "context", "fix the login bug"]);
        assert!(!cli.json);
        assert!(cli.quiet);
    }

    #[test]
    fn parse_json_flag_after_subcommand() {
        // global flags work after subcommand too
        let cli = Cli::parse_from(["sm", "context", "--json", "fix the login bug"]);
        assert!(cli.json);
    }

    #[test]
    fn parse_both_json_and_quiet() {
        let cli = Cli::parse_from(["sm", "--json", "--quiet", "distill"]);
        assert!(cli.json);
        assert!(cli.quiet);
    }

    #[test]
    fn parse_ingest_command() {
        let cli = Cli::parse_from([
            "sm", "ingest", "--task", "TASK-1", "--body", "did stuff", "--agent", "agent-0",
        ]);
        match cli.command {
            Some(Commands::Ingest(args)) => {
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
            Some(Commands::Ingest(args)) => {
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
            Some(Commands::Ingest(args)) => {
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
            Some(Commands::Ingest(args)) => {
                assert_eq!(args.success, vec!["rule-1", "rule-3"]);
                assert_eq!(args.harm, vec!["rule-2"]);
            }
            _ => panic!("expected Ingest command"),
        }
    }

    #[test]
    fn parse_distill_command() {
        let cli = Cli::parse_from(["sm", "distill"]);
        assert!(matches!(cli.command, Some(Commands::Distill)));
    }

    #[test]
    fn parse_status_command() {
        let cli = Cli::parse_from(["sm", "status"]);
        assert!(matches!(cli.command, Some(Commands::Status)));
    }

    #[test]
    fn parse_status_with_json_flag() {
        let cli = Cli::parse_from(["sm", "--json", "status"]);
        assert!(cli.json);
        assert!(matches!(cli.command, Some(Commands::Status)));
    }

    #[test]
    fn parse_rules_list() {
        let cli = Cli::parse_from(["sm", "rules", "list"]);
        match cli.command {
            Some(Commands::Rules(args)) => match args.action {
                Some(RulesAction::List(list)) => assert!(list.query.is_none()),
                _ => panic!("expected List"),
            },
            _ => panic!("expected Rules command"),
        }
    }

    #[test]
    fn parse_rules_no_subcommand_yields_none() {
        let cli = Cli::parse_from(["sm", "rules"]);
        match cli.command {
            Some(Commands::Rules(args)) => assert!(args.action.is_none()),
            _ => panic!("expected Rules command"),
        }
    }

    #[test]
    fn parse_rules_list_with_query() {
        let cli = Cli::parse_from(["sm", "rules", "list", "--query", "deploy"]);
        match cli.command {
            Some(Commands::Rules(args)) => match args.action {
                Some(RulesAction::List(list)) => assert_eq!(list.query.as_deref(), Some("deploy")),
                _ => panic!("expected List"),
            },
            _ => panic!("expected Rules command"),
        }
    }

    #[test]
    fn parse_rules_add() {
        let cli = Cli::parse_from(["sm", "rules", "add", "r1", "Always test"]);
        match cli.command {
            Some(Commands::Rules(args)) => match args.action {
                Some(RulesAction::Add(add)) => {
                    assert_eq!(add.id, "r1");
                    assert_eq!(add.rule, "Always test");
                    assert!(add.source.is_none());
                }
                _ => panic!("expected Add"),
            },
            _ => panic!("expected Rules command"),
        }
    }

    #[test]
    fn parse_rules_add_with_source() {
        let cli = Cli::parse_from(["sm", "rules", "add", "r1", "test", "--source", "postmortem"]);
        match cli.command {
            Some(Commands::Rules(args)) => match args.action {
                Some(RulesAction::Add(add)) => {
                    assert_eq!(add.source.as_deref(), Some("postmortem"));
                }
                _ => panic!("expected Add"),
            },
            _ => panic!("expected Rules command"),
        }
    }

    #[test]
    fn parse_rules_rm() {
        let cli = Cli::parse_from(["sm", "rules", "rm", "r1"]);
        match cli.command {
            Some(Commands::Rules(args)) => match args.action {
                Some(RulesAction::Rm(rm)) => assert_eq!(rm.id, "r1"),
                _ => panic!("expected Rm"),
            },
            _ => panic!("expected Rules command"),
        }
    }

    #[test]
    fn parse_rules_show() {
        let cli = Cli::parse_from(["sm", "rules", "show", "r1"]);
        match cli.command {
            Some(Commands::Rules(args)) => match args.action {
                Some(RulesAction::Show(show)) => assert_eq!(show.id, "r1"),
                _ => panic!("expected Show"),
            },
            _ => panic!("expected Rules command"),
        }
    }

    #[test]
    fn parse_prompt_command() {
        let cli = Cli::parse_from(["sm", "prompt"]);
        assert!(matches!(cli.command, Some(Commands::Prompt)));
    }

    #[test]
    fn parse_prompt_with_json_flag() {
        let cli = Cli::parse_from(["sm", "--json", "prompt"]);
        assert!(cli.json);
        assert!(matches!(cli.command, Some(Commands::Prompt)));
    }

    #[test]
    fn parse_rules_with_json_flag() {
        let cli = Cli::parse_from(["sm", "--json", "rules", "list"]);
        assert!(cli.json);
        assert!(matches!(cli.command, Some(Commands::Rules(_))));
    }

    #[test]
    fn no_args_yields_none_command() {
        // With arg_required_else_help=false, no subcommand parses as None.
        let cli = Cli::parse_from(["sm"]);
        assert!(cli.command.is_none());
    }

    #[test]
    fn try_parse_valid_args_succeeds() {
        let result = Cli::try_parse_from(["sm", "status"]);
        assert!(result.is_ok());
    }
}
