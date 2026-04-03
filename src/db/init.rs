use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::OnceLock;

const DEFAULT_DIR_NAME: &str = ".slashmem";
const DB_FILENAME: &str = "mem.db";
const PROJECTS_DIR: &str = "projects";

/// Stores an explicit project override set via `--project`.
static PROJECT_OVERRIDE: OnceLock<Option<String>> = OnceLock::new();

/// Set an explicit project name override (from `--project` CLI flag).
/// Must be called before any `base_dir()` / `open_db()` call.
pub fn set_project_override(project: Option<String>) {
    let _ = PROJECT_OVERRIDE.set(project);
}

/// Returns the root `~/.slashmem` directory (or `SLASHMEM_DIR` override).
fn global_root() -> PathBuf {
    if let Ok(dir) = std::env::var("SLASHMEM_DIR") {
        PathBuf::from(dir)
    } else {
        dirs::home_dir()
            .expect("could not determine home directory")
            .join(DEFAULT_DIR_NAME)
    }
}

/// Walk up from the current directory to find a `.git` entry.
/// If inside a git worktree (`.git` is a file), resolves to the main
/// repository root so that all worktrees share the same database.
fn detect_git_root() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let git_path = dir.join(".git");
        if git_path.exists() {
            // In a worktree, .git is a file containing "gitdir: <path>"
            // pointing to <main-repo>/.git/worktrees/<name>.
            // Resolve to the main repo root so all worktrees share one DB.
            if git_path.is_file() {
                if let Some(main_root) = resolve_worktree_root(&git_path) {
                    return Some(main_root);
                }
            }
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Given a `.git` file from a worktree, resolve the main repository root.
/// The file contains `gitdir: <main-repo>/.git/worktrees/<name>`.
fn resolve_worktree_root(git_file: &std::path::Path) -> Option<PathBuf> {
    let content = std::fs::read_to_string(git_file).ok()?;
    let gitdir = content.strip_prefix("gitdir: ")?.trim();
    let gitdir_path = if std::path::Path::new(gitdir).is_absolute() {
        PathBuf::from(gitdir)
    } else {
        // Relative path — resolve from the worktree directory
        git_file.parent()?.join(gitdir)
    };
    // gitdir points to <main-repo>/.git/worktrees/<name>
    // Walk up to find the .git directory, then its parent is the repo root
    let canonicalized = gitdir_path.canonicalize().ok()?;
    let mut ancestor = canonicalized.as_path();
    loop {
        if ancestor.file_name()?.to_str()? == ".git" {
            return ancestor.parent().map(PathBuf::from);
        }
        ancestor = ancestor.parent()?;
    }
}

/// Compute a stable short hash for a project path.
fn project_hash(path: &std::path::Path) -> String {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Returns the base directory for slashmem storage.
///
/// Resolution order:
/// 1. `SLASHMEM_DIR` env var → used as-is (backwards compatible)
/// 2. `--project <name>` flag → `~/.slashmem/projects/<name>/`
/// 3. Auto-detect git root → `~/.slashmem/projects/<hash>/`
/// 4. Fallback → `~/.slashmem/` (global)
pub fn base_dir() -> PathBuf {
    // SLASHMEM_DIR takes absolute priority (backwards compat)
    if std::env::var("SLASHMEM_DIR").is_ok() {
        return global_root();
    }

    let root = global_root();

    // Explicit --project override
    if let Some(Some(name)) = PROJECT_OVERRIDE.get() {
        return root.join(PROJECTS_DIR).join(name);
    }

    // Auto-detect git repo
    if let Some(git_root) = detect_git_root() {
        let hash = project_hash(&git_root);
        let project_dir = root.join(PROJECTS_DIR).join(&hash);
        // Write a metadata file so we can map hash → path for `projects list`
        if let Ok(()) = std::fs::create_dir_all(&project_dir) {
            let meta_path = project_dir.join("project.meta");
            if !meta_path.exists() {
                let _ = std::fs::write(&meta_path, git_root.display().to_string());
            }
        }
        return project_dir;
    }

    // Fallback: global database
    root
}

/// Returns the path to the SQLite database file.
pub fn db_path() -> PathBuf {
    base_dir().join(DB_FILENAME)
}

/// Ensures the base directory exists, then opens (or creates) the SQLite database.
pub fn open_db() -> crate::error::Result<rusqlite::Connection> {
    let dir = base_dir();
    std::fs::create_dir_all(&dir)?;
    Ok(rusqlite::Connection::open(db_path())?)
}

/// Holds information about a known project.
#[derive(Debug)]
pub struct ProjectInfo {
    pub name: String,
    pub path: String,
    pub db_path: String,
}

/// List all known projects stored under `~/.slashmem/projects/`.
pub fn list_projects() -> Vec<ProjectInfo> {
    let root = global_root().join(PROJECTS_DIR);
    let mut projects = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&root) {
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let name = dir
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();

            let meta_path = dir.join("project.meta");
            let path = std::fs::read_to_string(&meta_path).unwrap_or_default();

            let db = dir.join(DB_FILENAME);
            let db_path = db.display().to_string();

            projects.push(ProjectInfo {
                name,
                path,
                db_path,
            });
        }
    }
    projects
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
        // When running inside a git repo, base_dir will detect it and use projects/<hash>
        // so we just check it's under ~/.slashmem
        assert!(dir.starts_with(home.join(".slashmem")));
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

    #[test]
    fn project_hash_is_deterministic() {
        let path = PathBuf::from("/tmp/my-project");
        let h1 = project_hash(&path);
        let h2 = project_hash(&path);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 16);
    }

    #[test]
    fn project_hash_differs_for_different_paths() {
        let h1 = project_hash(&PathBuf::from("/tmp/project-a"));
        let h2 = project_hash(&PathBuf::from("/tmp/project-b"));
        assert_ne!(h1, h2);
    }
}
