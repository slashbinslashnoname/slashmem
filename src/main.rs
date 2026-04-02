pub mod cli;
pub mod confidence;
pub mod db;
pub mod error;
pub mod output;

use clap::Parser;
use cli::{Cli, Commands};

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Context(args) => cmd_context(args),
        Commands::Ingest(args) => cmd_ingest(args),
        Commands::Distill => cmd_distill(),
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn cmd_context(_args: cli::ContextArgs) -> error::Result<()> {
    // TODO: implement in slashmem-6188acc3-1su.1
    let out = output::ContextOutput::default();
    println!("{}", serde_json::to_string(&out).unwrap());
    Ok(())
}

fn cmd_ingest(_args: cli::IngestArgs) -> error::Result<()> {
    // TODO: implement in slashmem-6188acc3-1su.2
    let out = output::IngestOutput::default();
    println!("{}", serde_json::to_string(&out).unwrap());
    Ok(())
}

fn cmd_distill() -> error::Result<()> {
    // TODO: implement in slashmem-6188acc3-1su.3
    let out = output::DistillOutput::default();
    println!("{}", serde_json::to_string(&out).unwrap());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_parses() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    #[test]
    fn cmd_context_stub_returns_ok() {
        let args = cli::ContextArgs {
            description: "test".into(),
            json: false,
        };
        assert!(cmd_context(args).is_ok());
    }

    #[test]
    fn cmd_ingest_stub_returns_ok() {
        let args = cli::IngestArgs {
            task: "T-1".into(),
            body: "body".into(),
            agent: "a".into(),
            success: vec![],
            harm: vec![],
        };
        assert!(cmd_ingest(args).is_ok());
    }

    #[test]
    fn cmd_distill_stub_returns_ok() {
        assert!(cmd_distill().is_ok());
    }
}
