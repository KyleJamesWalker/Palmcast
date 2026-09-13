use std::collections::{HashMap, HashSet};
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

/// What survives a restart. Slides are absent on purpose: they are rebuilt from
/// the markdown on load, so a change to the parser cannot bring back a deck
/// that no longer matches its source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSession {
    pub id: String,
    pub owner_token: String,
    /// Absent in a file written before co-hosts existed. A session restored
    /// without one is given a fresh token rather than an empty one, which would
    /// otherwise let a blank token edit the deck.
    #[serde(default)]
    pub cohost_token: String,
    pub markdown: String,
    pub current: usize,
    pub rev: u64,
    #[serde(default)]
    pub votes: HashMap<usize, HashMap<String, Choice>>,
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
    /// How long the room had already sat idle when it was saved. A restart must
    /// not hand an expired room a fresh life, so the age travels with it.
    #[serde(default)]
    pub idle_seconds: u64,
}

/// What one voter chose on one slide.
///
/// A file written before answers could hold more than one option stores a bare
/// number. Reading it as either shape means an upgrade keeps the votes instead
/// of refusing the file or dropping the round.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Choice {
    One(usize),
    Several(Vec<usize>),
}

impl Choice {
    pub fn into_vec(self) -> Vec<usize> {
        match self {
            Choice::One(option) => vec![option],
            Choice::Several(options) => options,
        }
    }
}

impl From<Vec<usize>> for Choice {
    fn from(options: Vec<usize>) -> Self {
        Choice::Several(options)
    }
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
///
/// The temporary name is unique per call, by counter rather than by clock. The
/// periodic save and the save on shutdown can overlap, and any shared name lets
/// one rename the other's file out from under it. A nanosecond timestamp was
/// not enough: two saves in the same microsecond collided.
pub fn save(path: &Path, sessions: &[PersistedSession]) -> io::Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    // A clock is not a source of uniqueness: two saves in the same microsecond
    // picked the same name, and one renamed the other's file away.
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let ticket = NEXT.fetch_add(1, Ordering::Relaxed);
    let temporary = path.with_extension(format!("tmp{}-{ticket}", std::process::id()));
    let json = serde_json::to_vec(sessions)?;
    std::fs::write(&temporary, &json)?;
    restrict(&temporary)?;
    // A failed rename must not leave the temporary behind.
    if let Err(error) = std::fs::rename(&temporary, path) {
        std::fs::remove_file(&temporary).ok();
        return Err(error);
    }
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
            cohost_token: "co".into(),
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
            idle_seconds: 0,
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

    #[test]
    fn concurrent_saves_leave_a_readable_file_and_no_litter() {
        let dir = std::env::temp_dir().join(format!("pc-race-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("state.json");

        std::thread::scope(|scope| {
            for n in 0..8 {
                let path = path.clone();
                scope.spawn(move || {
                    let mut item = sample(&format!("room{n}"));
                    item.markdown = "x".repeat(200_000);
                    save(&path, &[item]).unwrap();
                });
            }
        });

        assert_eq!(
            load(&path).unwrap().len(),
            1,
            "the state file did not survive"
        );
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temporary files were left behind");
        std::fs::remove_dir_all(&dir).ok();
    }
}
