use crate::model::{ArtifactIndex, ComponentIndexEntry, Node, Summary, SUPPORTED_LANGUAGES};
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

pub const SESSION_TTL_SECONDS: i64 = 2 * 60 * 60;
pub const SESSION_LOCK_TIMEOUT_SECONDS: u64 = 120;
const SESSION_LOCK_RETRY_MILLIS: u64 = 50;
pub const REPOSITORY_WIKI_ID: &str = "repo";

pub fn change_wiki_id(change_id: &str) -> Result<String> {
    let Some((base, head)) = change_id.split_once("..") else {
        return Err(anyhow!("invalid change ID: {change_id}"));
    };
    if !matches!(base.len(), 40 | 64)
        || base.len() != head.len()
        || !base.bytes().all(|byte| byte.is_ascii_hexdigit())
        || !head.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(anyhow!("invalid full-SHA change ID: {change_id}"));
    }
    let canonical_change_id = change_id.to_ascii_lowercase();
    let digest = Sha256::digest(canonical_change_id.as_bytes());
    Ok(format!("change_{digest:x}"))
}

fn edition_wiki_id(output_dir: &Path) -> Result<String> {
    let Some(changes) = output_dir.parent() else {
        return Ok(REPOSITORY_WIKI_ID.to_string());
    };
    let is_change_output = changes.file_name().and_then(|name| name.to_str()) == Some("changes")
        && changes
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            == Some(".repowiki");
    if is_change_output {
        let change_id = output_dir
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| anyhow!("change edition path has no UTF-8 change ID"))?;
        change_wiki_id(change_id)
    } else {
        Ok(REPOSITORY_WIKI_ID.to_string())
    }
}

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Cross-process serialization for operations that read and then mutate a
/// session. The lock file lives outside the session directory so session
/// cleanup cannot remove it while a command still owns the lock.
pub struct SessionLock {
    file: File,
    repo_path: PathBuf,
    session_id: String,
}

impl SessionLock {
    pub fn acquire(repo_path: &Path, session_id: &str) -> Result<Self> {
        validate_session_id(session_id)?;
        let lock_root = session_storage_root(repo_path).join("session-locks");
        fs::create_dir_all(&lock_root)?;
        let path = lock_root.join(format!("{session_id}.lock"));
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .with_context(|| format!("open session lock {}", path.display()))?;
        let deadline = Instant::now() + Duration::from_secs(SESSION_LOCK_TIMEOUT_SECONDS);
        loop {
            match file.try_lock() {
                Ok(()) => {
                    return Ok(Self {
                        file,
                        repo_path: repo_path.to_path_buf(),
                        session_id: session_id.to_string(),
                    })
                }
                Err(TryLockError::WouldBlock) => {
                    if Instant::now() >= deadline {
                        return Err(anyhow!(
                            "session lock timeout after {} seconds: {}",
                            SESSION_LOCK_TIMEOUT_SECONDS,
                            path.display()
                        ));
                    }
                    thread::sleep(Duration::from_millis(SESSION_LOCK_RETRY_MILLIS));
                }
                Err(TryLockError::Error(error)) => {
                    return Err(error).with_context(|| format!("lock session {}", path.display()))
                }
            }
        }
    }

    /// The state of the session this lock serializes access to. An expired
    /// session is reclaimed and refused rather than handed out; a live one
    /// comes back marked as accessed, and the caller decides whether to
    /// persist that.
    pub fn load(&self) -> Result<SessionState> {
        let mut state = read_state(&self.repo_path, &self.session_id)?;
        if is_expired(&state) {
            cleanup(&self.repo_path, &self.session_id)?;
            return Err(anyhow!("session expired: {}", self.session_id));
        }
        state.touch();
        Ok(state)
    }
}

impl Drop for SessionLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionState {
    pub session_id: String,
    pub repo_path: String,
    pub output_dir: String,
    pub wiki_id: String,
    pub analyzed_commit: Option<String>,
    pub created_at: String,
    pub last_accessed: String,
    pub docs_written: usize,
    pub component_count: usize,
    pub leaf_count: usize,
    pub languages: Vec<String>,
}

impl SessionState {
    pub fn touch(&mut self) {
        self.last_accessed = Utc::now().to_rfc3339();
    }

    pub fn mark_write(&mut self) {
        self.docs_written += 1;
        self.touch();
    }
}

fn session_storage_root(repo_path: &Path) -> PathBuf {
    std::env::var_os("REPOWIKI_SESSION_REPO")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_path.to_path_buf())
        .join(".repowiki")
        .join(".state")
}

pub fn sessions_root(repo_path: &Path) -> PathBuf {
    session_storage_root(repo_path).join("sessions")
}

pub fn session_root(repo_path: &Path, session_id: &str) -> PathBuf {
    sessions_root(repo_path).join(session_id)
}

pub fn create(repo_path: &Path, output_dir: &Path) -> Result<SessionState> {
    let repo_path = repo_path
        .canonicalize()
        .with_context(|| format!("repository does not exist: {}", repo_path.display()))?;
    let output_dir = if output_dir.is_absolute() {
        output_dir.to_path_buf()
    } else {
        repo_path.join(output_dir)
    };
    fs::create_dir_all(&output_dir)?;
    let output_dir = output_dir
        .canonicalize()
        .with_context(|| format!("canonicalize output directory: {}", output_dir.display()))?;
    let wiki_id = edition_wiki_id(&output_dir)?;
    let root = sessions_root(&repo_path);
    fs::create_dir_all(&root)?;

    let mut session_id = Uuid::new_v4().simple().to_string()[..12].to_string();
    while session_root(&repo_path, &session_id).exists() {
        session_id = Uuid::new_v4().simple().to_string()[..12].to_string();
    }
    let workspace = session_root(&repo_path, &session_id);
    fs::create_dir_all(workspace.join("sources"))?;
    fs::create_dir_all(workspace.join("prompts"))?;
    fs::create_dir_all(workspace.join("history"))?;

    let now = Utc::now().to_rfc3339();
    let state = SessionState {
        session_id,
        repo_path: repo_path.to_string_lossy().into_owned(),
        output_dir: output_dir.to_string_lossy().into_owned(),
        wiki_id,
        analyzed_commit: None,
        created_at: now.clone(),
        last_accessed: now,
        docs_written: 0,
        component_count: 0,
        leaf_count: 0,
        languages: Vec::new(),
    };
    save_state(&state)?;
    Ok(state)
}

fn read_state(repo_path: &Path, session_id: &str) -> Result<SessionState> {
    validate_session_id(session_id)?;
    let path = session_root(repo_path, session_id).join("state.json");
    let contents =
        fs::read_to_string(&path).with_context(|| format!("session not found: {session_id}"))?;
    serde_json::from_str(&contents)
        .with_context(|| format!("invalid session state: {}", path.display()))
}

/// Read a session without taking the lock.
///
/// Another operation may replace what this returns the moment it reads it, so
/// the snapshot is only good for facts a session never changes — which
/// repository it belongs to. Anything that reads and then mutates goes through
/// [`with_locked_session`] or [`close_session`] instead.
pub fn peek(repo_path: &Path, session_id: &str) -> Result<SessionState> {
    read_state(repo_path, session_id)
}

/// Run `operation` with exclusive access to the session, then persist the
/// state it leaves behind. A failing operation writes nothing and leaves the
/// session exactly as it was.
pub fn with_locked_session<T, F>(repo_path: &Path, session_id: &str, operation: F) -> Result<T>
where
    F: FnOnce(&mut SessionState) -> Result<T>,
{
    let lock = SessionLock::acquire(repo_path, session_id)?;
    let mut state = lock.load()?;
    let value = operation(&mut state)?;
    save_state(&state)?;
    Ok(value)
}

/// Like [`with_locked_session`], but discards the session when `operation`
/// succeeds. Validation failures therefore keep it alive for another attempt.
pub fn close_session<T, F>(repo_path: &Path, session_id: &str, operation: F) -> Result<T>
where
    F: FnOnce(&mut SessionState) -> Result<T>,
{
    let lock = SessionLock::acquire(repo_path, session_id)?;
    let mut state = lock.load()?;
    let value = operation(&mut state)?;
    cleanup(repo_path, session_id)?;
    Ok(value)
}

fn save_state(state: &SessionState) -> Result<()> {
    let repo_path = Path::new(&state.repo_path);
    let path = session_root(repo_path, &state.session_id).join("state.json");
    write_json(&path, state)
}

fn session_lock_path(repo_path: &Path, session_id: &str) -> PathBuf {
    session_storage_root(repo_path)
        .join("session-locks")
        .join(format!("{session_id}.lock"))
}

/// Remove `dir` when it holds no entries; returns whether it is gone.
fn remove_if_empty(dir: &Path) -> bool {
    if dir.is_dir()
        && fs::read_dir(dir)
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(false)
    {
        let _ = fs::remove_dir(dir);
    }
    !dir.exists()
}

pub fn cleanup(repo_path: &Path, session_id: &str) -> Result<()> {
    validate_session_id(session_id)?;
    let root = session_root(repo_path, session_id);
    if root.exists() {
        fs::remove_dir_all(&root).with_context(|| format!("remove {}", root.display()))?;
    }
    // A platform that refuses to unlink an open handle can leave the lock file
    // behind. Nothing sweeps it: session IDs are fresh per session, so a stale
    // lock can never be mistaken for a live one.
    let _ = fs::remove_file(session_lock_path(repo_path, session_id));
    let storage = session_storage_root(repo_path);
    remove_if_empty(&storage.join("sessions"));
    remove_if_empty(&storage.join("session-locks"));
    remove_if_empty(&storage);
    Ok(())
}

pub fn is_expired(state: &SessionState) -> bool {
    chrono::DateTime::parse_from_rfc3339(&state.last_accessed)
        .map(|time| (Utc::now() - time.with_timezone(&Utc)).num_seconds() > SESSION_TTL_SECONDS)
        .unwrap_or(false)
}

pub fn write_analysis_files(
    state: &mut SessionState,
    nodes: &std::collections::BTreeMap<String, Node>,
    leaf_nodes: &[String],
    summary: &Summary,
    artifact_index: &ArtifactIndex,
) -> Result<()> {
    let root = session_root(Path::new(&state.repo_path), &state.session_id);
    let entries: Vec<ComponentIndexEntry> = nodes
        .values()
        .map(|node| ComponentIndexEntry {
            id: node.id.clone(),
            name: node.name.clone(),
            file_path: node.file_path.clone(),
            relative_path: node.relative_path.clone(),
            language: node.language.clone(),
            component_type: node.component_type.clone(),
            start_line: node.start_line,
            end_line: node.end_line,
        })
        .collect();
    write_json(&root.join("component_index.json"), &entries)?;
    write_json(&root.join("leaf_nodes.json"), leaf_nodes)?;
    let mut language_counts = BTreeMap::new();
    for node in nodes.values() {
        if SUPPORTED_LANGUAGES.contains(&node.language.as_str()) {
            *language_counts
                .entry(node.language.clone())
                .or_insert(0usize) += 1;
        }
    }
    write_json(&root.join("languages.json"), &language_counts)?;
    write_json(&root.join("summary.json"), summary)?;
    write_json(&root.join("artifact_index.json"), artifact_index)?;
    write_json(&root.join("components.json"), nodes)?;
    for node in nodes.values() {
        let source_path = root.join("sources").join(safe_source_filename(&node.id));
        let header = format!(
            "// Component: {}\n// Language: {}\n",
            node.id, node.language
        );
        write_text(&source_path, &(header + &node.source_code))?;
    }
    state.component_count = nodes.len();
    state.leaf_count = leaf_nodes.len();
    state.languages = summary.languages.clone();
    state.analyzed_commit = summary.analyzed_commit.clone();
    save_state(state)
}

pub fn write_json<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<()> {
    let contents = serde_json::to_string_pretty(value)? + "\n";
    write_text(path, &contents)
}

pub fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let contents =
        fs::read_to_string(path).with_context(|| format!("read JSON file {}", path.display()))?;
    serde_json::from_str(&contents).with_context(|| format!("parse JSON file {}", path.display()))
}

pub fn write_text(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let counter = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temporary = path.with_extension(format!("{}.{}.tmp", std::process::id(), counter));
    fs::write(&temporary, contents).with_context(|| format!("write {}", temporary.display()))?;
    fs::rename(&temporary, path).with_context(|| format!("commit {}", path.display()))?;
    Ok(())
}

pub fn safe_source_filename(component_id: &str) -> String {
    let mut sanitized = String::with_capacity(component_id.len());
    for ch in component_id.chars().take(180) {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.' {
            sanitized.push(ch);
        } else {
            sanitized.push('_');
            sanitized.push('_');
        }
    }
    let mut hash = Sha256::new();
    hash.update(component_id.as_bytes());
    let digest = format!("{:x}", hash.finalize());
    format!("{}_{}.src", sanitized, &digest[..8])
}

pub fn session_file(state: &SessionState, relative: &str) -> Result<PathBuf> {
    validate_session_id(&state.session_id)?;
    let relative = Path::new(relative);
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(anyhow!("unsafe session path: {}", relative.display()));
    }
    Ok(session_root(Path::new(&state.repo_path), &state.session_id).join(relative))
}

pub fn validate_session_id(session_id: &str) -> Result<()> {
    if session_id.is_empty()
        || session_id.len() > 64
        || !session_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(anyhow!("invalid session id"));
    }
    Ok(())
}

pub fn output_dir(state: &SessionState) -> PathBuf {
    PathBuf::from(&state.output_dir)
}

pub fn module_tree_path(state: &SessionState) -> PathBuf {
    output_dir(state).join("module_tree.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn change_namespace_hashes_the_complete_canonical_range() {
        let id =
            "0000000000000000000000000000000000000000..1111111111111111111111111111111111111111";
        assert_eq!(
            change_wiki_id(id).unwrap(),
            "change_654789463c0d55ef744ab82809415fad2e9a1a3d07bfd270bacccf7eefb6d3cb"
        );
        assert_eq!(
            change_wiki_id(&id.to_ascii_uppercase()).unwrap(),
            change_wiki_id(id).unwrap()
        );
        assert!(change_wiki_id("base..head").is_err());
    }

    #[test]
    fn change_output_paths_receive_a_change_namespace() {
        let id =
            "0000000000000000000000000000000000000000..1111111111111111111111111111111111111111";
        let output = Path::new("/tmp/repo/.repowiki/changes").join(id);
        assert_eq!(
            edition_wiki_id(&output).unwrap(),
            "change_654789463c0d55ef744ab82809415fad2e9a1a3d07bfd270bacccf7eefb6d3cb"
        );
        assert_eq!(
            edition_wiki_id(Path::new("/tmp/repo/.repowiki")).unwrap(),
            REPOSITORY_WIKI_ID
        );
    }
}
