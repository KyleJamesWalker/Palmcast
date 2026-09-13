use std::collections::{HashMap, HashSet};
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// What survives a restart. Slides are absent on purpose: they are rebuilt from
/// the markdown on load, so a change to the parser cannot bring back a deck
/// that no longer matches its source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSession {
    pub id: String,
    pub owner_token: String,
    pub markdown: String,
    pub current: usize,
    pub rev: u64,
    #[serde(default)]
    pub votes: HashMap<usize, HashMap<String, usize>>,
    #[serde(default)]
    pub revealed: HashSet<usize>,
    #[serde(default)]
    pub questions: Vec<PersistedQuestion>,
    #[serde(default)]
    pub next_question_id: u64,
    #[serde(default)]
    pub names: HashMap<String, String>,
    #[serde(default)]
    pub participants: HashSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedQuestion {
    pub id: u64,
    pub text: String,
    pub answered: bool,
    #[serde(default)]
    pub voters: HashSet<String>,
}

/// Writes through a temporary file in the same directory and renames it, so a
/// stop midway through leaves the previous state rather than half of this one.
pub fn save(path: &Path, sessions: &[PersistedSession]) -> io::Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("tmp");
    let json = serde_json::to_vec(sessions)?;
    std::fs::write(&temporary, &json)?;
    restrict(&temporary)?;
    std::fs::rename(&temporary, path)?;
    Ok(())
}

pub fn load(path: &Path) -> io::Result<Vec<PersistedSession>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

/// The file holds presenter tokens, which are the only thing standing between a
/// reader and control of every room on the instance.
#[cfg(unix)]
fn restrict(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn restrict(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(id: &str) -> PersistedSession {
        PersistedSession {
            id: id.to_string(),
            owner_token: "tok".into(),
            markdown: "# One\n\n---\n\n# Two".into(),
            current: 1,
            rev: 3,
            votes: HashMap::new(),
            revealed: HashSet::new(),
            questions: vec![PersistedQuestion {
                id: 1,
                text: "why".into(),
                answered: false,
                voters: HashSet::from(["sam".to_string()]),
            }],
            next_question_id: 2,
            names: HashMap::from([("sam".to_string(), "Sam".to_string())]),
            participants: HashSet::from(["sam".to_string()]),
        }
    }

    #[test]
    fn a_saved_state_loads_back_the_same() {
        let dir = std::env::temp_dir().join(format!("pc-{}", std::process::id()));
        let path = dir.join("state.json");
        save(&path, &[sample("abc123")]).unwrap();

        let back = load(&path).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].id, "abc123");
        assert_eq!(back[0].current, 1);
        assert_eq!(back[0].questions[0].text, "why");
        assert_eq!(back[0].names["sam"], "Sam");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_file_is_an_empty_start_not_an_error() {
        let path = std::env::temp_dir().join("palmcast-does-not-exist.json");
        std::fs::remove_file(&path).ok();
        assert!(load(&path).unwrap().is_empty());
    }

    #[test]
    fn a_corrupt_file_reports_an_error_rather_than_panicking() {
        let dir = std::env::temp_dir().join(format!("pc-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("state.json");
        std::fs::write(&path, b"{not json").unwrap();
        assert!(load(&path).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn the_state_file_is_not_world_readable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("pc-perm-{}", std::process::id()));
        let path = dir.join("state.json");
        save(&path, &[sample("abc123")]).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o077,
            0,
            "state file is readable by others: {mode:o}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
