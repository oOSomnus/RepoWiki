use crate::model::{
    ArtifactIndex, ComponentIndexEntry, ModuleTree, Node, Summary, SUPPORTED_LANGUAGES,
};
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

pub const SESSION_TTL_SECONDS: i64 = 2 * 60 * 60;
pub const MAX_SESSIONS: usize = 10;
pub const SESSION_LOCK_TIMEOUT_SECONDS: u64 = 120;
const SESSION_LOCK_RETRY_MILLIS: u64 = 50;

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Cross-process serialization for operations that read and then mutate a
/// session. The lock file lives outside the session directory so session
/// cleanup cannot remove it while a command still owns the lock.
pub struct SessionLock {
    file: File,
}

impl SessionLock {
    pub fn acquire(repo_path: &Path, session_id: &str) -> Result<Self> {
        validate_session_id(session_id)?;
        let lock_root = repo_path.join(".codewiki").join("session-locks");
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
                Ok(()) => return Ok(Self { file }),
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
    pub analyzed_commit: Option<String>,
    pub created_at: String,
    pub last_accessed: String,
    pub docs_written: usize,
    pub component_count: usize,
    pub leaf_count: usize,
    pub languages: Vec<String>,
    #[serde(default)]
    pub closed: bool,
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

pub fn sessions_root(repo_path: &Path) -> PathBuf {
    repo_path.join(".codewiki").join("sessions")
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
    let root = sessions_root(&repo_path);
    fs::create_dir_all(&root)?;
    prune(&repo_path)?;

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
        analyzed_commit: None,
        created_at: now.clone(),
        last_accessed: now,
        docs_written: 0,
        component_count: 0,
        leaf_count: 0,
        languages: Vec::new(),
        closed: false,
    };
    save_state(&state)?;
    Ok(state)
}

pub fn load(repo_path: &Path, session_id: &str) -> Result<SessionState> {
    validate_session_id(session_id)?;
    let state_path = session_root(repo_path, session_id).join("state.json");
    if !state_path.is_file() {
        return Err(anyhow!("session not found: {session_id}"));
    }
    let _lock = SessionLock::acquire(repo_path, session_id)?;
    load_unlocked(repo_path, session_id)
}

pub fn load_unlocked(repo_path: &Path, session_id: &str) -> Result<SessionState> {
    validate_session_id(session_id)?;
    let path = session_root(repo_path, session_id).join("state.json");
    let contents =
        fs::read_to_string(&path).with_context(|| format!("session not found: {}", session_id))?;
    let mut state: SessionState = serde_json::from_str(&contents)
        .with_context(|| format!("invalid session state: {}", path.display()))?;
    if is_expired(&state) {
        cleanup(repo_path, session_id)?;
        return Err(anyhow!("session expired: {}", session_id));
    }
    state.touch();
    save_state(&state)?;
    Ok(state)
}

pub fn with_locked_session<T, F>(repo_path: &Path, session_id: &str, operation: F) -> Result<T>
where
    F: FnOnce(&mut SessionState) -> Result<T>,
{
    let _lock = SessionLock::acquire(repo_path, session_id)?;
    let mut state = load_unlocked(repo_path, session_id)?;
    operation(&mut state)
}

pub fn save_state(state: &SessionState) -> Result<()> {
    let repo_path = Path::new(&state.repo_path);
    let path = session_root(repo_path, &state.session_id).join("state.json");
    write_json(&path, state)
}

pub fn cleanup(repo_path: &Path, session_id: &str) -> Result<()> {
    validate_session_id(session_id)?;
    let root = session_root(repo_path, session_id);
    if root.exists() {
        fs::remove_dir_all(&root).with_context(|| format!("remove {}", root.display()))?;
    }
    let sessions_dir = root.parent().map(Path::to_path_buf);
    if let Some(sessions_dir) = sessions_dir {
        if sessions_dir.exists()
            && fs::read_dir(&sessions_dir)
                .map(|mut entries| entries.next().is_none())
                .unwrap_or(false)
        {
            let _ = fs::remove_dir(&sessions_dir);
            if let Some(base_dir) = sessions_dir.parent() {
                if base_dir.exists()
                    && fs::read_dir(base_dir)
                        .map(|mut entries| entries.next().is_none())
                        .unwrap_or(false)
                {
                    let _ = fs::remove_dir(base_dir);
                }
            }
        }
    }
    Ok(())
}

pub fn prune(repo_path: &Path) -> Result<()> {
    let root = sessions_root(repo_path);
    if !root.exists() {
        return Ok(());
    }
    let mut sessions = Vec::new();
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let path = entry.path().join("state.json");
        if let Ok(contents) = fs::read_to_string(&path) {
            if let Ok(state) = serde_json::from_str::<SessionState>(&contents) {
                if is_expired(&state) {
                    let _ = fs::remove_dir_all(entry.path());
                } else {
                    sessions.push(state);
                }
            }
        }
    }
    sessions.sort_by(|a, b| a.last_accessed.cmp(&b.last_accessed));
    if sessions.len() > MAX_SESSIONS {
        let remove_count = sessions.len() - MAX_SESSIONS;
        for state in sessions.into_iter().take(remove_count) {
            let _ = fs::remove_dir_all(session_root(repo_path, &state.session_id));
        }
    }
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

pub fn read_text(path: &Path) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("read {}", path.display()))
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

pub fn safe_session_path(repo_path: &Path, session_id: &str, relative: &Path) -> Result<PathBuf> {
    validate_session_id(session_id)?;
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
    Ok(session_root(repo_path, session_id).join(relative))
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

pub fn repo_path(state: &SessionState) -> PathBuf {
    PathBuf::from(&state.repo_path)
}

pub fn session_value_path(state: &SessionState, name: &str) -> PathBuf {
    session_root(Path::new(&state.repo_path), &state.session_id).join(name)
}

pub fn read_value(state: &SessionState, name: &str) -> Result<Value> {
    read_json(&session_value_path(state, name))
}

pub fn write_value(state: &SessionState, name: &str, value: &Value) -> Result<()> {
    write_json(&session_value_path(state, name), value)
}

pub fn module_tree_path(state: &SessionState) -> PathBuf {
    output_dir(state).join("module_tree.json")
}

pub fn read_module_tree(state: &SessionState) -> Result<ModuleTree> {
    read_json(&module_tree_path(state))
}
