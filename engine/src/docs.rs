use crate::model::{Metadata, Module, ModuleTree, Statistics};
use crate::session::{self, SessionState};
use anyhow::{anyhow, Context, Result};
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

const RESERVED_STEMS: &[&str] = &[
    "overview",
    "module_tree",
    "first_module_tree",
    "metadata",
    "index",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteResult {
    pub path: String,
    pub created: bool,
    pub mermaid: MermaidReport,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MermaidReport {
    pub blocks: usize,
    pub balanced: bool,
    pub validator: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeSaveResult {
    pub path: String,
    pub first_path: Option<String>,
    pub processing_order_path: String,
    pub validation_path: String,
    pub module_count: usize,
    pub leaf_count: usize,
    pub unmatched_component_ids: Vec<String>,
    pub leftover_candidate_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ProcessingItem {
    pub module: String,
    pub path: Vec<String>,
    pub is_leaf: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<String>,
    pub components: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ProcessingItemInput {
    #[serde(default)]
    module: Option<String>,
    #[serde(default)]
    module_name: Option<String>,
    #[serde(default)]
    path: Option<Value>,
    #[serde(default)]
    is_leaf: bool,
    #[serde(default)]
    children: Vec<String>,
    #[serde(default)]
    components: Vec<String>,
}

impl<'de> Deserialize<'de> for ProcessingItem {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let input = ProcessingItemInput::deserialize(deserializer)?;
        let module = input
            .module
            .or(input.module_name)
            .ok_or_else(|| serde::de::Error::custom("processing item has no module"))?;
        let path = match input.path {
            Some(Value::Array(path)) => serde_json::from_value(Value::Array(path))
                .map_err(|error| serde::de::Error::custom(error.to_string()))?,
            Some(Value::String(_)) | None => vec![module.clone()],
            Some(value) => {
                return Err(serde::de::Error::custom(format!(
                    "processing item path must be an array, got {value}"
                )))
            }
        };
        Ok(Self {
            module,
            path,
            is_leaf: input.is_leaf,
            children: input.children,
            components: input.components,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EditOperation {
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default, alias = "old_str")]
    pub old: Option<String>,
    #[serde(default, alias = "new_str")]
    pub new: Option<String>,
    #[serde(default, alias = "insert_line")]
    pub line: Option<usize>,
    #[serde(default)]
    pub text: Option<String>,
}

pub fn write_document(
    state: &mut SessionState,
    requested: &str,
    content: &str,
) -> Result<WriteResult> {
    let path = document_path(state, requested)?;
    if path.exists() {
        return Err(anyhow!("document already exists: {}", path.display()));
    }
    let mermaid = validate_mermaid(content);
    session::write_text(&path, content)?;
    state.mark_write();
    session::save_state(state)?;
    Ok(WriteResult {
        path: path.to_string_lossy().into_owned(),
        created: true,
        mermaid,
    })
}

pub fn edit_document(
    state: &mut SessionState,
    requested: &str,
    operations: &[EditOperation],
) -> Result<WriteResult> {
    let path = document_path(state, requested)?;
    let mut content = session::read_text(&path)?;
    let history =
        session::session_root(Path::new(&state.repo_path), &state.session_id).join("history");
    let mut history_stack = load_history(&history, &path)?;
    let mut pending_history = Vec::new();
    let mut consumed_history = Vec::new();
    for operation in operations {
        let kind = operation
            .kind
            .as_deref()
            .or(operation.command.as_deref())
            .unwrap_or_default();
        match kind {
            "str_replace" | "replace" => {
                let old = operation.old.as_deref().unwrap_or_default();
                let new = operation.new.as_deref().unwrap_or_default();
                let count = content.matches(old).count();
                let expected = 1usize;
                if old.is_empty() || count != expected {
                    return Err(anyhow!(
                        "str_replace requires one match for {}, found {}",
                        requested,
                        count
                    ));
                }
                push_pending_history(&mut history_stack, &mut pending_history, &content);
                content = content.replacen(old, new, 1);
            }
            "insert" => {
                let line = operation.line.unwrap_or(0);
                let text = operation
                    .text
                    .as_deref()
                    .or(operation.new.as_deref())
                    .unwrap_or_default();
                push_pending_history(&mut history_stack, &mut pending_history, &content);
                let mut lines: Vec<String> = content.split('\n').map(str::to_string).collect();
                let insert_line = line.min(lines.len());
                let inserted = text.split('\n').map(str::to_string).collect::<Vec<_>>();
                lines.splice(insert_line..insert_line, inserted);
                content = lines.join("\n");
            }
            "undo" => {
                let previous = history_stack
                    .pop()
                    .ok_or_else(|| anyhow!("no edit history for {}", requested))?;
                if let Some(history_path) = previous.path {
                    consumed_history.push(history_path);
                } else if let Some(index) = previous.pending_index {
                    pending_history[index] = None;
                }
                content = previous.content;
            }
            other => return Err(anyhow!("unsupported edit operation: {other}")),
        }
    }

    for history_path in consumed_history {
        if history_path.exists() {
            fs::remove_file(&history_path).map_err(|error| {
                anyhow!("remove edit history {}: {error}", history_path.display())
            })?;
        }
    }
    for (index, snapshot) in pending_history.into_iter().enumerate() {
        let Some(snapshot) = snapshot else {
            continue;
        };
        write_history_snapshot(&history, &path, &snapshot, index)?;
    }
    session::write_text(&path, &content)?;
    let mermaid = validate_mermaid(&content);
    state.mark_write();
    session::save_state(state)?;
    Ok(WriteResult {
        path: path.to_string_lossy().into_owned(),
        created: false,
        mermaid,
    })
}

pub fn view_document(state: &SessionState, requested: &str) -> Result<Value> {
    let path = document_path(state, requested)?;
    let content = session::read_text(&path)?;
    Ok(json!({
        "path": path,
        "content": content,
        "mermaid": validate_mermaid(&content),
    }))
}

pub fn save_module_tree(
    state: &SessionState,
    tree: &ModuleTree,
    first: bool,
) -> Result<TreeSaveResult> {
    let output = session::output_dir(state);
    fs::create_dir_all(&output)?;
    let tree_path = output.join("module_tree.json");
    session::write_json(&tree_path, tree)?;
    let first_path = if first {
        let path = output.join("first_module_tree.json");
        session::write_json(&path, tree)?;
        Some(path)
    } else {
        None
    };

    let nodes: BTreeMap<String, Value> =
        session::read_json(&session::session_value_path(state, "components.json"))?;
    let mut assigned = BTreeSet::new();
    let mut order = Vec::new();
    let mut module_count = 0usize;
    let mut leaf_count = 0usize;
    for (name, module) in tree {
        collect_processing(
            name,
            module,
            &[],
            &mut order,
            &mut assigned,
            &mut module_count,
            &mut leaf_count,
        );
    }
    let known_ids = nodes.keys().cloned().collect::<BTreeSet<_>>();
    let candidate_ids =
        session::read_json::<Vec<String>>(&session::session_value_path(state, "leaf_nodes.json"))?
            .into_iter()
            .collect::<BTreeSet<_>>();
    let unmatched = assigned.difference(&known_ids).cloned().collect::<Vec<_>>();
    let leftover = candidate_ids
        .difference(&assigned)
        .cloned()
        .collect::<Vec<_>>();
    let validation = json!({
        "valid": unmatched.is_empty(),
        "unmatched_ids": unmatched.clone(),
        "unmatched_count": unmatched.len(),
        "leftover_component_ids": leftover.clone(),
        "leftover_component_count": leftover.len(),
        "unmatched_component_ids": unmatched.clone(),
        "leftover_candidate_ids": leftover.clone(),
        "module_count": module_count,
        "leaf_count": leaf_count,
    });
    let root = session::session_root(Path::new(&state.repo_path), &state.session_id);
    let order_path = root.join("processing_order.json");
    let validation_path = root.join("module_tree_validation.json");
    session::write_json(&order_path, &order)?;
    session::write_json(&validation_path, &validation)?;
    Ok(TreeSaveResult {
        path: tree_path.to_string_lossy().into_owned(),
        first_path: first_path.map(|path| path.to_string_lossy().into_owned()),
        processing_order_path: order_path.to_string_lossy().into_owned(),
        validation_path: validation_path.to_string_lossy().into_owned(),
        module_count,
        leaf_count,
        unmatched_component_ids: validation["unmatched_component_ids"]
            .as_array()
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        leftover_candidate_ids: validation["leftover_candidate_ids"]
            .as_array()
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
    })
}

pub fn read_tree_file(path: &Path) -> Result<ModuleTree> {
    session::read_json(path)
}

pub fn read_processing_order(state: &SessionState) -> Result<Vec<ProcessingItem>> {
    session::read_json(&session::session_value_path(state, "processing_order.json"))
}

pub fn finalize_metadata(state: &SessionState, model: &str) -> Result<Metadata> {
    let output = session::output_dir(state);
    let tree: ModuleTree = if output.join("module_tree.json").exists() {
        session::read_json(&output.join("module_tree.json"))?
    } else {
        ModuleTree::new()
    };
    let mut files_generated = Vec::new();
    for required in ["overview.md", "module_tree.json", "first_module_tree.json"] {
        if output.join(required).exists() {
            files_generated.push(required.to_string());
        }
    }
    let mut max_depth = 0usize;
    let mut count = 0usize;
    collect_metadata(
        &tree,
        &output,
        1,
        &mut count,
        &mut max_depth,
        &mut files_generated,
    );
    let metadata = Metadata {
        generation_info: crate::model::GenerationInfo {
            timestamp: Utc::now().to_rfc3339(),
            main_model: model.to_string(),
            generator_version: env!("CARGO_PKG_VERSION").to_string(),
            repo_path: state.repo_path.clone(),
            commit_id: state.analyzed_commit.clone(),
        },
        statistics: Statistics {
            total_components: state.component_count,
            leaf_nodes: state.leaf_count,
            max_depth,
        },
        files_generated,
        last_update: output
            .join("update_record.json")
            .is_file()
            .then(|| session::read_json(&output.join("update_record.json")))
            .transpose()?,
    };
    session::write_json(&output.join("metadata.json"), &metadata)?;
    Ok(metadata)
}

/// Verify the file-side completion contract before a session is removed.
///
/// The host agent owns Markdown generation, but the engine is the only place
/// that knows which pages the saved tree requires.  Closing an incomplete
/// session would otherwise make a failed generation indistinguishable from a
/// successful one because the workspace is cleaned immediately afterwards.
pub fn validate_documentation(state: &SessionState) -> Result<()> {
    let output = session::output_dir(state);
    for required in ["overview.md", "module_tree.json", "first_module_tree.json"] {
        if !output.join(required).is_file() {
            return Err(anyhow!("incomplete documentation: missing {required}"));
        }
    }

    let validation_path = session::session_value_path(state, "module_tree_validation.json");
    let validation: Value = session::read_json(&validation_path).with_context(|| {
        format!(
            "incomplete documentation: missing {}",
            validation_path.display()
        )
    })?;
    for field in ["unmatched_ids", "leftover_component_ids"] {
        if validation[field]
            .as_array()
            .is_some_and(|values| !values.is_empty())
        {
            return Err(anyhow!(
                "incomplete documentation: module tree validation field '{field}' is non-empty"
            ));
        }
    }

    let tree: ModuleTree = session::read_json(&output.join("module_tree.json"))?;
    let mut expected = BTreeSet::new();
    collect_expected_pages(&tree, &mut expected);
    let missing = expected
        .into_iter()
        .filter(|page| !output.join(page).is_file())
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(anyhow!(
            "incomplete documentation: missing module pages {}",
            missing.join(", ")
        ));
    }
    Ok(())
}

pub fn validate_mermaid(content: &str) -> MermaidReport {
    let mut blocks = 0usize;
    let mut open = false;
    for line in content.lines() {
        let trimmed = line.trim().to_ascii_lowercase();
        if trimmed.starts_with("```mermaid") {
            blocks += 1;
            open = true;
        } else if open && trimmed.starts_with("```") {
            open = false;
        }
    }
    MermaidReport {
        blocks,
        balanced: !open,
        validator: "side-channel-best-effort".to_string(),
    }
}

fn collect_processing(
    name: &str,
    module: &Module,
    parent_path: &[String],
    order: &mut Vec<ProcessingItem>,
    assigned: &mut BTreeSet<String>,
    module_count: &mut usize,
    leaf_count: &mut usize,
) {
    let mut current_path = parent_path.to_vec();
    current_path.push(name.to_string());
    let mut child_names = Vec::new();
    for (child_name, child) in &module.children {
        child_names.push(child_name.clone());
        collect_processing(
            child_name,
            child,
            &current_path,
            order,
            assigned,
            module_count,
            leaf_count,
        );
    }
    assigned.extend(module.components.iter().cloned());
    let is_leaf = module.children.is_empty();
    if is_leaf {
        *leaf_count += 1;
    }
    *module_count += 1;
    order.push(ProcessingItem {
        module: name.to_string(),
        path: current_path,
        is_leaf,
        children: child_names,
        components: module.components.clone(),
    });
}

fn collect_metadata(
    tree: &ModuleTree,
    output: &Path,
    depth: usize,
    count: &mut usize,
    max_depth: &mut usize,
    files: &mut Vec<String>,
) {
    for (name, module) in tree {
        *count += 1;
        *max_depth = (*max_depth).max(depth);
        let file = module_page_filename(name);
        if output.join(&file).exists() && !files.iter().any(|item| item == &file) {
            files.push(file);
        }
        collect_metadata(&module.children, output, depth + 1, count, max_depth, files);
    }
}

fn collect_expected_pages(tree: &ModuleTree, expected: &mut BTreeSet<String>) {
    for (name, module) in tree {
        expected.insert(module_page_filename(name));
        collect_expected_pages(&module.children, expected);
    }
}

fn document_path(state: &SessionState, requested: &str) -> Result<PathBuf> {
    let requested = requested.trim_start_matches("docs/");
    let path = Path::new(requested);
    if requested.is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(anyhow!("unsafe document path: {requested}"));
    }
    let file = if path.extension().is_none() {
        path.with_extension("md")
    } else {
        path.to_path_buf()
    };
    let output = session::output_dir(state);
    let candidate = output.join(file);
    let canonical_parent = output.canonicalize().unwrap_or(output);
    let parent = candidate.parent().unwrap_or(&candidate);
    if parent.exists() {
        let canonical = parent.canonicalize()?;
        if !canonical.starts_with(&canonical_parent) {
            return Err(anyhow!("document path escapes output directory"));
        }
    }
    Ok(candidate)
}

fn sanitize_module_name(name: &str) -> String {
    let mut result = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '&' {
            result.push(ch);
        } else {
            result.push('_');
        }
    }
    let result = if result.is_empty() {
        "module".to_string()
    } else {
        result
    };
    if RESERVED_STEMS.contains(&result.as_str()) {
        format!("{}_module", result)
    } else {
        result
    }
}

/// Return the flat Markdown filename used for a module everywhere in the
/// generation and update workflows.
pub fn module_page_filename(name: &str) -> String {
    format!("{}.md", sanitize_module_name(name))
}

fn history_key(path: &Path, timestamp: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(path.to_string_lossy().as_bytes());
    hasher.update(timestamp.as_bytes());
    format!("{:x}", hasher.finalize())
}

struct HistoryEntry {
    path: Option<PathBuf>,
    content: String,
    pending_index: Option<usize>,
}

fn push_pending_history(
    stack: &mut Vec<HistoryEntry>,
    pending: &mut Vec<Option<String>>,
    content: &str,
) {
    let index = pending.len();
    pending.push(Some(content.to_string()));
    stack.push(HistoryEntry {
        path: None,
        content: content.to_string(),
        pending_index: Some(index),
    });
}

fn load_history(history: &Path, path: &Path) -> Result<Vec<HistoryEntry>> {
    if !history.exists() {
        return Ok(Vec::new());
    }
    let mut candidates = Vec::new();
    for entry in fs::read_dir(history)? {
        let entry = entry?;
        if entry.path().extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let value: Value = match session::read_json(&entry.path()) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if value["path"].as_str() == Some(&path.to_string_lossy()) {
            let content = match value["content"].as_str() {
                Some(content) => content.to_string(),
                None => continue,
            };
            candidates.push((
                value["timestamp"].as_str().map(str::to_string),
                entry.metadata()?.modified().ok(),
                entry.path(),
                content,
            ));
        }
    }
    candidates.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.cmp(&right.1))
            .then_with(|| left.2.to_string_lossy().cmp(&right.2.to_string_lossy()))
    });
    Ok(candidates
        .into_iter()
        .map(|(_, _, path, content)| HistoryEntry {
            path: Some(path),
            content,
            pending_index: None,
        })
        .collect())
}

fn write_history_snapshot(history: &Path, path: &Path, content: &str, index: usize) -> Result<()> {
    let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true);
    let mut sequence = index;
    let history_path = loop {
        let candidate = history.join(format!(
            "{}.json",
            history_key(path, &format!("{timestamp}:{sequence}"))
        ));
        if !candidate.exists() {
            break candidate;
        }
        sequence += 1;
    };
    session::write_json(
        &history_path,
        &json!({
            "path": path.to_string_lossy(),
            "content": content,
            "timestamp": timestamp,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mermaid_report_is_best_effort() {
        let report = validate_mermaid("```mermaid\ngraph TD\nA-->B\n```\n");
        assert_eq!(report.blocks, 1);
        assert!(report.balanced);
    }

    #[test]
    fn reserved_module_names_are_safe() {
        assert_eq!(sanitize_module_name("overview"), "overview_module");
        assert_eq!(sanitize_module_name("Core Services"), "Core_Services");
        assert_eq!(module_page_filename("overview"), "overview_module.md");
    }
}
