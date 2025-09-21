//! Persistence layer for the global, append-only *message history* file.
//!
//! The history is stored at `~/.codex/history.jsonl` with **one JSON object per
//! line** so that it can be efficiently appended to and parsed with standard
//! JSON-Lines tooling. Each record has the following schema:
//!
//! ````text
//! {"conversation_id":"<uuid>","ts":<unix_seconds>,"text":"<message>"}
//! ````
//!
//! To minimise the chance of interleaved writes when multiple processes are
//! appending concurrently, callers should *prepare the full line* (record +
//! trailing `\n`) and write it with a **single `write(2)` system call** while
//! the file descriptor is opened with the `O_APPEND` flag. POSIX guarantees
//! that writes up to `PIPE_BUF` bytes are atomic in that case.

#[cfg(unix)]
use once_cell::sync::Lazy;
#[cfg(unix)]
use std::collections::HashMap;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::Result;
use std::io::Write;
use std::path::PathBuf;
#[cfg(unix)]
use std::sync::Mutex;

use serde::Deserialize;
use serde::Serialize;

use std::time::Duration;
use tokio::fs;
use tokio::io::AsyncReadExt;

use crate::config::Config;
use crate::config_types::HistoryPersistence;

use codex_protocol::mcp_protocol::ConversationId;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Filename that stores the message history inside `~/.codex`.
const HISTORY_FILENAME: &str = "history.jsonl";

const MAX_RETRIES: usize = 10;
const RETRY_SLEEP: Duration = Duration::from_millis(100);

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct HistoryEntry {
    pub session_id: String,
    pub ts: u64,
    pub text: String,
}

#[cfg(unix)]
#[derive(Clone)]
struct HistoryCache {
    path: PathBuf,
    entries: Vec<HistoryEntry>,
    byte_offset: u64,
}

#[cfg(unix)]
impl HistoryCache {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            entries: Vec::new(),
            byte_offset: 0,
        }
    }

    fn reset(&mut self, path: PathBuf) {
        self.path = path;
        self.entries.clear();
        self.byte_offset = 0;
    }
}

#[cfg(unix)]
static HISTORY_CACHE: Lazy<Mutex<HashMap<u64, HistoryCache>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

fn history_filepath(config: &Config) -> PathBuf {
    let mut path = config.codex_home.clone();
    path.push(HISTORY_FILENAME);
    path
}

/// Append a `text` entry associated with `conversation_id` to the history file. Uses
/// advisory file locking to ensure that concurrent writes do not interleave,
/// which entails a small amount of blocking I/O internally.
pub(crate) async fn append_entry(
    text: &str,
    conversation_id: &ConversationId,
    config: &Config,
) -> Result<()> {
    match config.history.persistence {
        HistoryPersistence::SaveAll => {
            // Save everything: proceed.
        }
        HistoryPersistence::None => {
            // No history persistence requested.
            return Ok(());
        }
    }

    // TODO: check `text` for sensitive patterns

    // Resolve `~/.codex/history.jsonl` and ensure the parent directory exists.
    let path = history_filepath(config);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    // Compute timestamp (seconds since the Unix epoch).
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| std::io::Error::other(format!("system clock before Unix epoch: {e}")))?
        .as_secs();

    // Construct the JSON line first so we can write it in a single syscall.
    let entry = HistoryEntry {
        session_id: conversation_id.to_string(),
        ts,
        text: text.to_string(),
    };
    let mut line = serde_json::to_string(&entry)
        .map_err(|e| std::io::Error::other(format!("failed to serialise history entry: {e}")))?;
    line.push('\n');

    // Open in append-only mode.
    let mut options = OpenOptions::new();
    options.append(true).read(true).create(true);
    #[cfg(unix)]
    {
        options.mode(0o600);
    }

    let mut history_file = options.open(&path)?;

    // Ensure permissions.
    ensure_owner_only_permissions(&history_file).await?;

    // Perform a blocking write under an advisory write lock using std::fs.
    tokio::task::spawn_blocking(move || -> Result<()> {
        // Retry a few times to avoid indefinite blocking when contended.
        for _ in 0..MAX_RETRIES {
            match history_file.try_lock() {
                Ok(()) => {
                    // While holding the exclusive lock, write the full line.
                    history_file.write_all(line.as_bytes())?;
                    history_file.flush()?;
                    return Ok(());
                }
                Err(std::fs::TryLockError::WouldBlock) => {
                    std::thread::sleep(RETRY_SLEEP);
                }
                Err(e) => return Err(e.into()),
            }
        }

        Err(std::io::Error::new(
            std::io::ErrorKind::WouldBlock,
            "could not acquire exclusive lock on history file after multiple attempts",
        ))
    })
    .await??;

    Ok(())
}

/// Asynchronously fetch the history file's *identifier* (inode on Unix) and
/// the current number of entries by counting newline characters.
pub(crate) async fn history_metadata(config: &Config) -> (u64, usize) {
    let path = history_filepath(config);

    #[cfg(unix)]
    let log_id = {
        use std::os::unix::fs::MetadataExt;
        // Obtain metadata (async) to get the identifier.
        let meta = match fs::metadata(&path).await {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (0, 0),
            Err(_) => return (0, 0),
        };
        meta.ino()
    };
    #[cfg(not(unix))]
    let log_id = 0u64;

    // Open the file.
    let mut file = match fs::File::open(&path).await {
        Ok(f) => f,
        Err(_) => return (log_id, 0),
    };

    // Count newline bytes.
    let mut buf = [0u8; 8192];
    let mut count = 0usize;
    loop {
        match file.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => {
                count += buf[..n].iter().filter(|&&b| b == b'\n').count();
            }
            Err(_) => return (log_id, 0),
        }
    }

    (log_id, count)
}

/// Given a `log_id` (on Unix this is the file's inode number) and a zero-based
/// `offset`, return the corresponding `HistoryEntry` if the identifier matches
/// the current history file **and** the requested offset exists. Any I/O or
/// parsing errors are logged and result in `None`.
///
/// Note this function is not async because it uses a sync advisory file
/// locking API.
#[cfg(unix)]
pub(crate) fn lookup(log_id: u64, offset: usize, config: &Config) -> Option<HistoryEntry> {
    use std::os::unix::fs::MetadataExt;

    let path = history_filepath(config);
    let mut file: File = match OpenOptions::new().read(true).open(&path) {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!(error = %e, "failed to open history file");
            return None;
        }
    };

    let metadata = match file.metadata() {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(error = %e, "failed to stat history file");
            return None;
        }
    };

    if metadata.ino() != log_id {
        return None;
    }

    // Open & lock file for reading using a shared lock.
    // Retry a few times to avoid indefinite blocking.
    for _ in 0..MAX_RETRIES {
        let lock_result = file.try_lock_shared();

        match lock_result {
            Ok(()) => {
                let mut cache_guard = match HISTORY_CACHE.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => poisoned.into_inner(),
                };

                let cache_entry = cache_guard
                    .entry(log_id)
                    .or_insert_with(|| HistoryCache::new(path.clone()));

                if cache_entry.path != path || metadata.len() < cache_entry.byte_offset {
                    cache_entry.reset(path.clone());
                }

                let needed = offset + 1;
                if cache_entry.entries.len() < needed
                    && let Err(e) = load_history_entries(cache_entry, &mut file, needed)
                {
                    tracing::warn!(error = %e, "failed to read history file");
                    return None;
                }

                return cache_entry.entries.get(offset).cloned();
            }
            Err(std::fs::TryLockError::WouldBlock) => {
                std::thread::sleep(RETRY_SLEEP);
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to acquire shared lock on history file");
                return None;
            }
        }
    }

    None
}

#[cfg(unix)]
fn load_history_entries(
    cache: &mut HistoryCache,
    file: &mut File,
    needed: usize,
) -> std::io::Result<()> {
    use std::io::BufRead;
    use std::io::BufReader;
    use std::io::Seek;
    use std::io::SeekFrom;

    file.seek(SeekFrom::Start(cache.byte_offset))?;
    let mut reader = BufReader::new(file);
    let mut buf = String::new();

    while cache.entries.len() < needed {
        buf.clear();
        let read = reader.read_line(&mut buf)?;
        if read == 0 {
            break;
        }
        let line = buf.trim_end_matches(['\n', '\r']);
        let entry: HistoryEntry = serde_json::from_str(line).map_err(std::io::Error::other)?;
        cache.byte_offset += read as u64;
        cache.entries.push(entry);
    }

    Ok(())
}

/// Fallback stub for non-Unix systems: currently always returns `None`.
#[cfg(not(unix))]
pub(crate) fn lookup(log_id: u64, offset: usize, config: &Config) -> Option<HistoryEntry> {
    let _ = (log_id, offset, config);
    None
}

/// On Unix systems ensure the file permissions are `0o600` (rw-------). If the
/// permissions cannot be changed the error is propagated to the caller.
#[cfg(unix)]
async fn ensure_owner_only_permissions(file: &File) -> Result<()> {
    let metadata = file.metadata()?;
    let current_mode = metadata.permissions().mode() & 0o777;
    if current_mode != 0o600 {
        let mut perms = metadata.permissions();
        perms.set_mode(0o600);
        let perms_clone = perms.clone();
        let file_clone = file.try_clone()?;
        tokio::task::spawn_blocking(move || file_clone.set_permissions(perms_clone)).await??;
    }
    Ok(())
}

#[cfg(not(unix))]
async fn ensure_owner_only_permissions(_file: &File) -> Result<()> {
    // For now, on non-Unix, simply succeed.
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::config::ConfigOverrides;
    use crate::config::ConfigToml;
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::os::unix::fs::MetadataExt;
    use std::path::Path;
    use tempfile::TempDir;

    fn reset_history_cache() {
        if let Ok(mut guard) = HISTORY_CACHE.lock() {
            guard.clear();
        }
    }

    fn write_entries(path: &Path, entries: &[HistoryEntry]) {
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)
            .unwrap_or_else(|e| panic!("create history file: {e}"));
        for entry in entries {
            let mut line = serde_json::to_string(entry)
                .unwrap_or_else(|e| panic!("serialize history entry: {e}"));
            line.push('\n');
            file.write_all(line.as_bytes())
                .unwrap_or_else(|e| panic!("write history entry: {e}"));
        }
    }

    #[test]
    fn sequential_lookups_use_cached_offsets() {
        reset_history_cache();
        let codex_home = TempDir::new().unwrap_or_else(|e| panic!("codex home tempdir: {e}"));
        let config = Config::load_from_base_config_with_overrides(
            ConfigToml::default(),
            ConfigOverrides::default(),
            codex_home.path().to_path_buf(),
        )
        .unwrap_or_else(|e| panic!("default config: {e}"));

        let history_path = history_filepath(&config);
        let base_entries = vec![
            HistoryEntry {
                session_id: "s1".to_string(),
                ts: 1,
                text: "first".to_string(),
            },
            HistoryEntry {
                session_id: "s2".to_string(),
                ts: 2,
                text: "second".to_string(),
            },
            HistoryEntry {
                session_id: "s3".to_string(),
                ts: 3,
                text: "third".to_string(),
            },
        ];
        write_entries(&history_path, &base_entries);

        let log_id = std::fs::metadata(&history_path)
            .unwrap_or_else(|e| panic!("metadata: {e}"))
            .ino();

        for (idx, expected) in base_entries.iter().enumerate() {
            let entry = lookup(log_id, idx, &config).unwrap_or_else(|| panic!("entry for offset"));
            assert_eq!(entry.session_id, expected.session_id);
            assert_eq!(entry.ts, expected.ts);
            assert_eq!(entry.text, expected.text);
        }

        let new_entry = HistoryEntry {
            session_id: "s4".to_string(),
            ts: 4,
            text: "fourth".to_string(),
        };
        let mut append_file = OpenOptions::new()
            .append(true)
            .open(&history_path)
            .unwrap_or_else(|e| panic!("open for append: {e}"));
        let mut line = serde_json::to_string(&new_entry)
            .unwrap_or_else(|e| panic!("serialize new entry: {e}"));
        line.push('\n');
        append_file
            .write_all(line.as_bytes())
            .unwrap_or_else(|e| panic!("append history entry: {e}"));

        let entry = lookup(log_id, base_entries.len(), &config)
            .unwrap_or_else(|| panic!("entry after append"));
        assert_eq!(entry.session_id, new_entry.session_id);
        assert_eq!(entry.ts, new_entry.ts);
        assert_eq!(entry.text, new_entry.text);
    }
}
