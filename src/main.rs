pub mod confidence;
pub mod db;

use clap::Parser;

/// slashmem — a local memory store
#[derive(Parser)]
#[command(name = "sm", version, about)]
struct Cli {
    /// Placeholder subcommand
    #[arg(short, long)]
    version_flag: bool,
}

fn main() {
    // Verify dependencies link correctly
    let _cli = Cli::parse();

    // Verify rusqlite links
    let _conn = rusqlite::Connection::open_in_memory().expect("sqlite works");

    // Verify serde/serde_json link
    let val = serde_json::json!({"status": "ok"});
    println!("{}", val);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_parses() {
        // Verify the CLI struct is valid clap config
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    #[test]
    fn sqlite_in_memory() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute("CREATE TABLE test (id INTEGER PRIMARY KEY)", [])
            .unwrap();
    }

    #[test]
    fn serde_roundtrip() {
        #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
        struct Sample {
            key: String,
        }
        let s = Sample {
            key: "val".to_string(),
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: Sample = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }
}
