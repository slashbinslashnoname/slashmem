use std::path::PathBuf;

const DEFAULT_DIR_NAME: &str = ".slashmem";
const DB_FILENAME: &str = "mem.db";

/// Returns the base directory for slashmem storage.
///
/// If `SLASHMEM_DIR` is set, uses that path directly.
/// Otherwise defaults to `~/.slashmem/`.
pub fn base_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("SLASHMEM_DIR") {
        PathBuf::from(dir)
    } else {
        dirs::home_dir()
            .expect("could not determine home directory")
            .join(DEFAULT_DIR_NAME)
    }
}

/// Returns the path to the SQLite database file.
pub fn db_path() -> PathBuf {
    base_dir().join(DB_FILENAME)
}

/// Ensures the base directory exists, then opens (or creates) the SQLite database.
pub fn open_db() -> rusqlite::Result<rusqlite::Connection> {
    let dir = base_dir();
    std::fs::create_dir_all(&dir).expect("failed to create slashmem directory");
    let path = dir.join(DB_FILENAME);
    rusqlite::Connection::open(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Serial lock so env-var mutations don't race across tests.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn base_dir_defaults_to_home() {
        let _guard = ENV_LOCK.lock().unwrap();
        // Clear the override so we get the default
        // SAFETY: test is serialized via ENV_LOCK
        unsafe { std::env::remove_var("SLASHMEM_DIR") };

        let dir = base_dir();
        let home = dirs::home_dir().unwrap();
        assert_eq!(dir, home.join(".slashmem"));
    }

    #[test]
    fn base_dir_respects_env_override() {
        let _guard = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("SLASHMEM_DIR", tmp.path()) };

        let dir = base_dir();
        assert_eq!(dir, tmp.path());

        unsafe { std::env::remove_var("SLASHMEM_DIR") };
    }

    #[test]
    fn db_path_under_base_dir() {
        let _guard = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("SLASHMEM_DIR", tmp.path()) };

        let path = db_path();
        assert_eq!(path, tmp.path().join("mem.db"));

        unsafe { std::env::remove_var("SLASHMEM_DIR") };
    }

    #[test]
    fn open_db_creates_dir_and_file() {
        let _guard = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("sub").join("dir");
        unsafe { std::env::set_var("SLASHMEM_DIR", &nested) };

        let conn = open_db().unwrap();
        assert!(nested.join("mem.db").exists());
        conn.execute("CREATE TABLE smoke (id INTEGER PRIMARY KEY)", [])
            .unwrap();

        unsafe { std::env::remove_var("SLASHMEM_DIR") };
    }
}
