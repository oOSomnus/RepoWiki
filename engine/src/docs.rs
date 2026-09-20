use crate::model::{
    Metadata, Module, ModuleTree, Node, Statistics, Summary, DEFAULT_CLUSTER_BATCH_SIZE,
    DEFAULT_MAX_TOKEN_PER_LEAF_MODULE, DEFAULT_MAX_TOKEN_PER_MODULE,
};
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
    pub max_depth: usize,
    pub quality_valid: bool,
    pub quality_errors: Vec<String>,
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

    let nodes: BTreeMap<String, Node> =
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
    let quality = assess_tree_quality(state, tree, &nodes, &candidate_ids);
    let quality_errors = quality["quality_errors"]
        .as_array()
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let unresolved_fallbacks = unresolved_cluster_fallbacks(state)?;
    let mut quality_errors = quality_errors;
    quality_errors.extend(unresolved_fallbacks.iter().map(|record| {
        format!(
            "unresolved clustering fallback for {} component(s) in {} scope",
            record["input_ids"].as_array().map_or(0, Vec::len),
            record["scope"].as_str().unwrap_or("unknown")
        )
    }));
    let quality_valid =
        quality["quality_valid"].as_bool().unwrap_or(false) && unresolved_fallbacks.is_empty();
    let validation = json!({
        "valid": unmatched.is_empty(),
        "complete": unmatched.is_empty() && leftover.is_empty() && quality_valid,
        "unmatched_ids": unmatched.clone(),
        "unmatched_count": unmatched.len(),
        "leftover_component_ids": leftover.clone(),
        "leftover_component_count": leftover.len(),
        "unmatched_component_ids": unmatched.clone(),
        "leftover_candidate_ids": leftover.clone(),
        "module_count": module_count,
        "leaf_count": leaf_count,
        "max_depth": quality["max_depth"],
        "quality_valid": quality_valid,
        "quality_errors": quality_errors,
        "unresolved_cluster_fallbacks": unresolved_fallbacks,
        "oversized_leaf_modules": quality["oversized_leaf_modules"],
        "oversized_leaf_warnings": quality["oversized_leaf_warnings"],
        "orphaned_candidate_ids": quality["orphaned_candidate_ids"],
        "tree_relationship_errors": quality["tree_relationship_errors"],
        "depth_errors": quality["depth_errors"],
        "quality_limits": quality["limits"],
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
        max_depth: quality["max_depth"].as_u64().unwrap_or_default() as usize,
        quality_valid,
        quality_errors,
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

/// Apply one host-agent clustering response to a working tree.
///
/// The reference implementation mutates the tree while recursively invoking
/// its clustering routine.  RepoWiki keeps the model call in the host, so this
/// function is the deterministic half of that protocol: parse the marked
/// response, validate the exact input IDs, merge the groups at the requested
/// scope, and structurally rescue anything the model omitted.
pub fn apply_cluster_response(
    state: &SessionState,
    tree: &mut ModuleTree,
    response: &str,
    input_ids: &[String],
    scope: &str,
    parent_path: &[String],
) -> Result<Value> {
    if scope != "repo" && scope != "module" {
        return Err(anyhow!("cluster scope must be 'repo' or 'module'"));
    }
    if scope == "module" && parent_path.is_empty() {
        return Err(anyhow!(
            "module clustering requires a non-empty parent path"
        ));
    }
    if input_ids.is_empty() {
        return Err(anyhow!(
            "cluster input must contain at least one component ID"
        ));
    }
    let nodes: BTreeMap<String, Node> =
        session::read_json(&session::session_value_path(state, "components.json"))?;
    let requested = input_ids.iter().cloned().collect::<BTreeSet<_>>();
    let unknown_input = requested
        .difference(&nodes.keys().cloned().collect())
        .cloned()
        .collect::<Vec<_>>();
    if !unknown_input.is_empty() {
        return Err(anyhow!(
            "cluster input contains unknown component IDs: {}",
            unknown_input.join(", ")
        ));
    }

    let mut diagnostics = Vec::new();
    let parsed = parse_grouped_components(response, &mut diagnostics);
    let mut groups = Vec::<(String, Module)>::new();
    let mut claimed = BTreeSet::new();
    let mut fallback_used = false;
    if let Some(parsed) = parsed {
        for (name, mut module) in parsed {
            let original = module.components.len();
            module.components.retain(|id| {
                if !requested.contains(id) {
                    diagnostics.push(format!(
                        "group '{name}' referenced component outside its input: {id}"
                    ));
                    return false;
                }
                if !claimed.insert(id.clone()) {
                    diagnostics.push(format!("component assigned more than once: {id}"));
                    return false;
                }
                true
            });
            if original != module.components.len() {
                diagnostics.push(format!(
                    "group '{name}' had {} invalid or duplicate component(s)",
                    original - module.components.len()
                ));
            }
            if !module.components.is_empty() {
                groups.push((name, module));
            }
        }
    }

    let missing = requested.difference(&claimed).cloned().collect::<Vec<_>>();
    if !missing.is_empty() {
        fallback_used = true;
        diagnostics.push(format!(
            "structural fallback assigned {} omitted component(s)",
            missing.len()
        ));
        let name = fallback_module_name(&missing, &nodes, tree);
        groups.push((
            name,
            Module {
                path: Some(common_path(&missing, &nodes)),
                components: missing,
                children: BTreeMap::new(),
            },
        ));
    }
    if groups.is_empty() {
        fallback_used = true;
        diagnostics.push("empty clustering response; created a structural fallback".to_string());
        let all = requested.into_iter().collect::<Vec<_>>();
        let name = fallback_module_name(&all, &nodes, tree);
        groups.push((
            name,
            Module {
                path: Some(common_path(&all, &nodes)),
                components: all,
                children: BTreeMap::new(),
            },
        ));
    }

    if scope == "repo" {
        merge_modules(tree, groups);
    } else {
        let parent = module_at_path_mut(tree, parent_path)
            .ok_or_else(|| anyhow!("module path not found: {}", parent_path.join("/")))?;
        for id in input_ids {
            if !parent.components.iter().any(|component| component == id) {
                parent.components.push(id.clone());
            }
        }
        merge_modules(&mut parent.children, groups);
        for depth in 1..parent_path.len() {
            if let Some(ancestor) = module_at_path_mut(tree, &parent_path[..depth]) {
                for id in input_ids {
                    if !ancestor.components.iter().any(|component| component == id) {
                        ancestor.components.push(id.clone());
                    }
                }
            }
        }
    }
    Ok(json!({
        "scope": scope,
        "parent_path": parent_path,
        "input_count": input_ids.len(),
        "group_count": if scope == "repo" { tree.len() } else { module_at_path(tree, parent_path).map(|module| module.children.len()).unwrap_or_default() },
        "diagnostics": diagnostics,
        "fallback_used": fallback_used,
    }))
}

/// Keep structural clustering fallbacks visible until the host successfully
/// retries the exact request.  The parser still returns a deterministic tree
/// for recovery, but a fallback is not allowed to become a final wiki by
/// accident.
pub fn record_cluster_diagnostics(
    state: &SessionState,
    input_ids: &[String],
    scope: &str,
    parent_path: &[String],
    diagnostics: &Value,
) -> Result<()> {
    let path = session::session_value_path(state, "cluster_diagnostics.json");
    let mut records: Vec<Value> = if path.is_file() {
        session::read_json(&path)?
    } else {
        Vec::new()
    };
    let key = cluster_request_key(input_ids, scope, parent_path);
    records.retain(|record| record["key"].as_str() != Some(&key));
    records.push(json!({
        "key": key,
        "scope": scope,
        "parent_path": parent_path,
        "input_ids": input_ids,
        "fallback_used": diagnostics["fallback_used"].as_bool().unwrap_or(false),
        "resolved": diagnostics["fallback_used"].as_bool() != Some(true),
        "diagnostics": diagnostics["diagnostics"].clone(),
    }));
    session::write_json(&path, &records)
}

fn unresolved_cluster_fallbacks(state: &SessionState) -> Result<Vec<Value>> {
    let path = session::session_value_path(state, "cluster_diagnostics.json");
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let records: Vec<Value> = session::read_json(&path)?;
    Ok(records
        .into_iter()
        .filter(|record| record["resolved"] != Value::Bool(true))
        .collect())
}

fn cluster_request_key(input_ids: &[String], scope: &str, parent_path: &[String]) -> String {
    let mut ids = input_ids.to_vec();
    ids.sort();
    format!("{scope}|{}|{}", parent_path.join("/"), ids.join("\\n"))
}

/// Apply a super-group response by making the existing top-level modules
/// children of newly named architectural parents.  Existing module pages and
/// aggregate IDs are preserved verbatim.
pub fn apply_super_group_response(tree: &mut ModuleTree, response: &str) -> Result<Value> {
    let mut diagnostics = Vec::new();
    let Some(grouping) = parse_grouped_modules(response, &mut diagnostics) else {
        return Ok(json!({
            "changed": false,
            "diagnostics": diagnostics,
            "top_level_count": tree.len(),
        }));
    };
    let original = tree.clone();
    let names = original.keys().cloned().collect::<BTreeSet<_>>();
    let mut assigned = BTreeSet::new();
    let mut result = ModuleTree::new();
    let mut consolidated = 0usize;
    for (name, info) in grouping {
        let members = info.children.keys().cloned().collect::<Vec<_>>();
        if members.len() < 2 {
            continue;
        }
        let mut valid = Vec::new();
        for member in members {
            if !names.contains(&member) {
                diagnostics.push(format!("unknown module in super-group '{name}': {member}"));
            } else if assigned.insert(member.clone()) {
                valid.push(member);
            } else {
                diagnostics.push(format!("module assigned more than once: {member}"));
            }
        }
        if valid.len() < 2 {
            continue;
        }
        let mut components = Vec::new();
        let mut seen = BTreeSet::new();
        let mut children = BTreeMap::new();
        for member in valid {
            let child = original
                .get(&member)
                .expect("validated module name")
                .clone();
            for id in &child.components {
                if seen.insert(id.clone()) {
                    components.push(id.clone());
                }
            }
            children.insert(member, child);
        }
        result.insert(
            name,
            Module {
                path: None,
                components,
                children,
            },
        );
        consolidated += 1;
    }
    for (name, module) in original {
        if !assigned.contains(&name) {
            result.insert(name, module);
        }
    }
    let changed = consolidated > 0 && result.len() < tree.len();
    if changed {
        *tree = result;
    }
    Ok(json!({
        "changed": changed,
        "consolidated_groups": consolidated,
        "top_level_count": tree.len(),
        "diagnostics": diagnostics,
    }))
}

/// Build the small context object consumed by overview prompts.  Components
/// are intentionally removed; overview agents should read child pages rather
/// than inline every leaf's source.
pub fn overview_context(
    tree: &ModuleTree,
    target_path: &[String],
    output_dir: &Path,
) -> Result<Value> {
    fn visit(
        modules: &ModuleTree,
        target_path: &[String],
        prefix: &[String],
        output_dir: &Path,
    ) -> Value {
        let mut result = serde_json::Map::new();
        let target_is_here = prefix == target_path;
        for (name, module) in modules {
            let mut object = serde_json::Map::new();
            object.insert(
                "path".to_string(),
                module
                    .path
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
            );
            let mut current = prefix.to_vec();
            current.push(name.clone());
            let is_target = current == target_path;
            object.insert(
                "is_target_for_overview_generation".to_string(),
                json!(is_target),
            );
            let children = visit(&module.children, target_path, &current, output_dir);
            if let Some(children_object) = children.as_object() {
                object.insert(
                    "children".to_string(),
                    Value::Object(children_object.clone()),
                );
            }
            if target_is_here {
                let docs_path = output_dir.join(module_page_filename(name));
                object.insert(
                    "docs_path".to_string(),
                    if docs_path.is_file() {
                        Value::String(docs_path.to_string_lossy().into_owned())
                    } else {
                        Value::Null
                    },
                );
            }
            result.insert(name.clone(), Value::Object(object));
        }
        Value::Object(result)
    }

    Ok(visit(tree, target_path, &[], output_dir))
}

/// Ensure artifact candidates are never lost between clustering and the final
/// tree.  The host may still attach them to a more specific module; this
/// rescue only adds IDs that are absent everywhere.
pub fn ensure_artifact_coverage(
    state: &SessionState,
    tree: &mut ModuleTree,
) -> Result<Vec<String>> {
    let nodes: BTreeMap<String, Node> =
        session::read_json(&session::session_value_path(state, "components.json"))?;
    let assigned = collect_leaf_tree_ids(tree);
    let missing = nodes
        .values()
        .filter(|node| node.component_type == "artifact" && !assigned.contains(&node.id))
        .map(|node| node.id.clone())
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(Vec::new());
    }
    let name = unique_module_name("Build, Deployment and Configuration", tree);
    tree.insert(
        name,
        Module {
            path: Some(common_path(&missing, &nodes)),
            components: missing.clone(),
            children: BTreeMap::new(),
        },
    );
    Ok(missing)
}

fn parse_grouped_components(
    response: &str,
    diagnostics: &mut Vec<String>,
) -> Option<BTreeMap<String, Module>> {
    parse_grouped_object(response, "GROUPED_COMPONENTS", diagnostics)
}

fn parse_grouped_modules(
    response: &str,
    diagnostics: &mut Vec<String>,
) -> Option<BTreeMap<String, Module>> {
    parse_grouped_object(response, "GROUPED_MODULES", diagnostics)
}

fn parse_grouped_object(
    response: &str,
    marker: &str,
    diagnostics: &mut Vec<String>,
) -> Option<BTreeMap<String, Module>> {
    let body = if let (Some(start), Some(end)) = (
        response.find(&format!("<{marker}>")),
        response.find(&format!("</{marker}>")),
    ) {
        &response[start + marker.len() + 2..end]
    } else {
        response
    };
    let body = body
        .trim()
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    if body.is_empty() {
        diagnostics.push(format!("empty {marker} response"));
        return None;
    }
    let value: Value = match serde_json::from_str(body) {
        Ok(value) => value,
        Err(error) => {
            diagnostics.push(format!("could not parse {marker} JSON: {error}"));
            return None;
        }
    };
    let Some(object) = value.as_object() else {
        diagnostics.push(format!("{marker} response must be a JSON object"));
        return None;
    };
    let mut result = BTreeMap::new();
    for (name, info) in object {
        let Some(info) = info.as_object() else {
            diagnostics.push(format!("group '{name}' is not an object"));
            continue;
        };
        result.insert(name.clone(), parse_module_value(info, diagnostics));
    }
    Some(result)
}

fn parse_module_value(
    info: &serde_json::Map<String, Value>,
    diagnostics: &mut Vec<String>,
) -> Module {
    let path = info.get("path").and_then(Value::as_str).map(str::to_string);
    let components = info
        .get("components")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(|value| {
                    value.as_str().map(str::to_string).or_else(|| {
                        diagnostics.push("component ID was not a string".to_string());
                        None
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let children_value = info.get("children").or_else(|| info.get("modules"));
    let mut children = BTreeMap::new();
    if let Some(children_object) = children_value.and_then(Value::as_object) {
        for (name, child) in children_object {
            if let Some(child) = child.as_object() {
                children.insert(name.clone(), parse_module_value(child, diagnostics));
            }
        }
    } else if let Some(modules) = children_value.and_then(Value::as_array) {
        for module in modules.iter().filter_map(Value::as_str) {
            children.insert(module.to_string(), Module::default());
        }
    }
    Module {
        path,
        components,
        children,
    }
}

fn merge_modules(target: &mut ModuleTree, groups: Vec<(String, Module)>) {
    for (name, mut incoming) in groups {
        if let Some(existing) = target.get_mut(&name) {
            for id in incoming.components.drain(..) {
                if !existing.components.contains(&id) {
                    existing.components.push(id);
                }
            }
            if existing.path.is_none() {
                existing.path = incoming.path.take();
            }
            let children = incoming.children.into_iter().collect::<Vec<_>>();
            merge_modules(&mut existing.children, children);
        } else {
            target.insert(name, incoming);
        }
    }
}

fn module_at_path<'a>(tree: &'a ModuleTree, path: &[String]) -> Option<&'a Module> {
    let (first, rest) = path.split_first()?;
    let mut module = tree.get(first)?;
    for name in rest {
        module = module.children.get(name)?;
    }
    Some(module)
}

fn module_at_path_mut<'a>(tree: &'a mut ModuleTree, path: &[String]) -> Option<&'a mut Module> {
    let (first, rest) = path.split_first()?;
    let mut module = tree.get_mut(first)?;
    for name in rest {
        module = module.children.get_mut(name)?;
    }
    Some(module)
}

fn collect_leaf_tree_ids(tree: &ModuleTree) -> BTreeSet<String> {
    fn visit(modules: &ModuleTree, ids: &mut BTreeSet<String>) {
        for module in modules.values() {
            if module.children.is_empty() {
                ids.extend(module.components.iter().cloned());
            } else {
                visit(&module.children, ids);
            }
        }
    }
    let mut ids = BTreeSet::new();
    visit(tree, &mut ids);
    ids
}

fn common_path(ids: &[String], nodes: &BTreeMap<String, Node>) -> String {
    let mut common: Option<Vec<&str>> = None;
    for id in ids {
        let Some(node) = nodes.get(id) else {
            continue;
        };
        let mut parts = node.relative_path.split('/').collect::<Vec<_>>();
        if parts.len() > 1 {
            parts.pop();
        }
        if let Some(existing) = &mut common {
            let length = existing
                .iter()
                .zip(parts.iter())
                .take_while(|(left, right)| left == right)
                .count();
            existing.truncate(length);
        } else {
            common = Some(parts);
        }
    }
    common
        .unwrap_or_default()
        .into_iter()
        .collect::<Vec<_>>()
        .join("/")
}

fn fallback_module_name(
    ids: &[String],
    nodes: &BTreeMap<String, Node>,
    tree: &ModuleTree,
) -> String {
    let path = common_path(ids, nodes);
    let base = path
        .rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or("Repository");
    unique_module_name(&format!("{base} Components"), tree)
}

fn unique_module_name(base: &str, tree: &ModuleTree) -> String {
    let base = sanitize_module_name(base);
    if !tree_contains_name(tree, &base) {
        return base;
    }
    let mut index = 2usize;
    loop {
        let candidate = format!("{base}_{index}");
        if !tree_contains_name(tree, &candidate) {
            return candidate;
        }
        index += 1;
    }
}

fn tree_contains_name(tree: &ModuleTree, name: &str) -> bool {
    tree.iter().any(|(module_name, module)| {
        module_name == name || tree_contains_name(&module.children, name)
    })
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
    let mut module_leaf_count = 0usize;
    collect_metadata(
        &tree,
        &output,
        1,
        &mut count,
        &mut max_depth,
        &mut module_leaf_count,
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
            analysis_leaf_candidates: state.leaf_count,
            leaf_nodes: module_leaf_count,
            module_count: count,
            max_depth,
        },
        files_generated,
        documentation_quality: session::read_json(&session::session_value_path(
            state,
            "documentation_validation.json",
        ))
        .ok(),
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
    let report = validate_documentation_report(state)?;
    if report["valid"] != Value::Bool(true) {
        let errors = report["errors"]
            .as_array()
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("; ")
            })
            .unwrap_or_else(|| "documentation quality gate failed".to_string());
        return Err(anyhow!("incomplete documentation: {errors}"));
    }
    Ok(())
}

/// Produce the file-side and semantic documentation report without deleting
/// the session.  This is intentionally a separate operation from the close
/// gate: the host agent and its review subagents need actionable diagnostics
/// before they can repair a page.
pub fn validate_documentation_report(state: &SessionState) -> Result<Value> {
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
    if validation["quality_valid"].as_bool() == Some(false)
        || validation["quality_errors"]
            .as_array()
            .is_some_and(|values| !values.is_empty())
    {
        let errors = validation["quality_errors"]
            .as_array()
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("; ")
            })
            .unwrap_or_else(|| "module tree quality gate failed".to_string());
        return Err(anyhow!(
            "incomplete documentation: module tree quality gate failed: {errors}"
        ));
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

    let nodes: BTreeMap<String, Node> =
        session::read_json(&session::session_value_path(state, "components.json"))
            .unwrap_or_default();
    let mut builder = DocumentationReportBuilder::new(&output, &nodes);
    builder.visit_modules(&tree);
    let overview_path = output.join("overview.md");
    let overview = fs::read_to_string(&overview_path).unwrap_or_default();
    builder.visit_overview(&tree, &overview);
    let DocumentationReportBuilder {
        pages,
        errors,
        page_count,
        valid_page_count,
        explanatory_page_count,
        mermaid_page_count,
        grounded_page_count,
        ..
    } = builder;

    let report = json!({
        "valid": errors.is_empty(),
        "errors": errors,
        "page_count": page_count,
        "valid_page_count": valid_page_count,
        "explanatory_page_count": explanatory_page_count,
        "mermaid_page_count": mermaid_page_count,
        "grounded_page_count": grounded_page_count,
        "pages": Value::Object(pages),
        "checks": {
            "source_grounding": true,
            "semantic_sections": true,
            "parent_child_links": true,
            "architecture_diagrams": true,
            "template_only_rejection": true,
        },
    });
    session::write_json(
        &session::session_value_path(state, "documentation_validation.json"),
        &report,
    )?;
    Ok(report)
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

struct DocumentationReportBuilder<'a> {
    output: &'a Path,
    nodes: &'a BTreeMap<String, Node>,
    pages: serde_json::Map<String, Value>,
    errors: Vec<String>,
    page_count: usize,
    valid_page_count: usize,
    explanatory_page_count: usize,
    mermaid_page_count: usize,
    grounded_page_count: usize,
}

impl<'a> DocumentationReportBuilder<'a> {
    fn new(output: &'a Path, nodes: &'a BTreeMap<String, Node>) -> Self {
        Self {
            output,
            nodes,
            pages: serde_json::Map::new(),
            errors: Vec::new(),
            page_count: 0,
            valid_page_count: 0,
            explanatory_page_count: 0,
            mermaid_page_count: 0,
            grounded_page_count: 0,
        }
    }

    fn visit_modules(&mut self, modules: &ModuleTree) {
        for (name, module) in modules {
            let page = module_page_filename(name);
            let path = self.output.join(&page);
            let content = fs::read_to_string(&path).unwrap_or_default();
            let required_links = module
                .children
                .keys()
                .map(|child| module_page_filename(child))
                .collect::<Vec<_>>();
            let result = assess_page(
                name,
                module,
                &content,
                self.nodes,
                module.children.is_empty(),
                false,
                &required_links,
            );
            self.record_page(&page, result);
            self.visit_modules(&module.children);
        }
    }

    fn visit_overview(&mut self, tree: &ModuleTree, content: &str) {
        let overview_links = tree
            .keys()
            .map(|name| module_page_filename(name))
            .collect::<Vec<_>>();
        let result = assess_page(
            "Repository overview",
            &Module::default(),
            content,
            self.nodes,
            false,
            true,
            &overview_links,
        );
        self.record_page("overview.md", result);
    }

    fn record_page(&mut self, page: &str, result: Value) {
        if result["valid"].as_bool() == Some(true) {
            self.valid_page_count += 1;
        }
        if result["explanatory"].as_bool() == Some(true) {
            self.explanatory_page_count += 1;
        }
        if result["mermaid_blocks"].as_u64().unwrap_or_default() > 0 {
            self.mermaid_page_count += 1;
        }
        if result["grounded_components"].as_u64().unwrap_or_default() > 0 {
            self.grounded_page_count += 1;
        }
        if let Some(page_errors) = result["errors"].as_array() {
            for error in page_errors.iter().filter_map(Value::as_str) {
                self.errors.push(format!("{page}: {error}"));
            }
        }
        self.page_count += 1;
        self.pages.insert(page.to_string(), result);
    }
}

/// Check whether a generated page contains an explanation grounded in the
/// analyzed repository. This is deliberately a small structural heuristic,
/// not an attempt to judge prose with another model. It catches the failure
/// mode where a host calls prompt get but then writes a fixed component list
/// or a one-line overview instead of using the model response.
fn assess_page(
    name: &str,
    module: &Module,
    content: &str,
    nodes: &BTreeMap<String, Node>,
    is_leaf: bool,
    is_overview: bool,
    required_links: &[String],
) -> Value {
    let headings = markdown_headings(content);
    let lower = content.to_ascii_lowercase();
    let mermaid = validate_mermaid(content);
    let prose_words = markdown_prose_word_count(content);
    let purpose = has_heading_term(&headings, &["purpose", "scope", "role"]);
    let architecture = has_heading_term(
        &headings,
        &[
            "architecture",
            "design",
            "data flow",
            "dataflow",
            "dependencies",
            "execution",
            "lifecycle",
            "component interaction",
            "behavior",
            "behaviour",
        ],
    );
    let responsibilities = has_heading_term(
        &headings,
        &["responsibilities", "interfaces", "usage", "error"],
    );
    let semantic_sections =
        usize::from(purpose) + usize::from(architecture) + usize::from(responsibilities);
    let prose_limit = if is_overview {
        50
    } else if is_leaf {
        60
    } else {
        50
    };
    let explanatory = prose_words >= prose_limit && semantic_sections >= 2;

    let mut grounded_components = 0usize;
    for component_id in &module.components {
        let Some(node) = nodes.get(component_id) else {
            continue;
        };
        let markers = [
            component_id.as_str(),
            node.relative_path.as_str(),
            node.file_path.as_str(),
            node.name.as_str(),
        ];
        if markers
            .iter()
            .filter(|marker| !marker.is_empty())
            .any(|marker| content.contains(marker))
        {
            grounded_components += 1;
        }
    }

    let template_sections = [
        "module location",
        "source files",
        "key components",
        "integration notes",
    ];
    let template_only = template_sections
        .iter()
        .all(|section| headings.iter().any(|heading| heading == section))
        && mermaid.blocks == 0
        && !architecture
        && !responsibilities;

    let mut page_errors = Vec::new();
    if content.trim().is_empty() {
        page_errors.push("page is empty".to_string());
    }
    if !purpose {
        page_errors.push("missing a semantic Purpose/Scope section".to_string());
    }
    if !architecture {
        page_errors
            .push("missing a semantic Architecture/Data flow/Dependencies section".to_string());
    }
    if prose_words < prose_limit {
        page_errors.push(format!(
            "contains only {prose_words} explanatory words; add source-grounded prose"
        ));
    }
    if is_leaf && grounded_components == 0 && !module.components.is_empty() {
        page_errors.push("does not mention any analyzed component or source path".to_string());
    }
    if !mermaid.balanced {
        page_errors.push("contains an unbalanced Mermaid fence".to_string());
    }
    if template_only {
        page_errors.push(
            "looks like a generated component-list template rather than an explanatory page"
                .to_string(),
        );
    }
    if !is_leaf && mermaid.blocks == 0 {
        page_errors
            .push("module overview needs at least one Mermaid architecture diagram".to_string());
    }
    if is_overview && mermaid.blocks == 0 {
        page_errors.push("repository overview needs an end-to-end Mermaid diagram".to_string());
    }
    let mut missing_links = Vec::new();
    for link in required_links {
        if !contains_markdown_link(content, link) {
            missing_links.push(link.clone());
        }
    }
    if !missing_links.is_empty() {
        page_errors.push(format!(
            "does not link to child/module pages: {}",
            missing_links.join(", ")
        ));
    }

    json!({
        "valid": page_errors.is_empty(),
        "role": if is_overview { "repository_overview" } else if is_leaf { "leaf" } else { "module_overview" },
        "module": name,
        "errors": page_errors,
        "explanatory": explanatory,
        "prose_words": prose_words,
        "semantic_sections": semantic_sections,
        "grounded_components": grounded_components,
        "component_count": module.components.len(),
        "mermaid_blocks": mermaid.blocks,
        "mermaid_balanced": mermaid.balanced,
        "missing_links": missing_links,
        "boilerplate_detected": template_only || lower.contains("this leaf documents a cohesive implementation area"),
    })
}

fn markdown_headings(content: &str) -> Vec<String> {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            let heading = trimmed.trim_start_matches('#');
            if heading.len() == trimmed.len() || heading.trim().is_empty() {
                None
            } else {
                Some(heading.trim().to_ascii_lowercase())
            }
        })
        .collect()
}

fn has_heading_term(headings: &[String], terms: &[&str]) -> bool {
    headings
        .iter()
        .any(|heading| terms.iter().any(|term| heading.contains(term)))
}

fn markdown_prose_word_count(content: &str) -> usize {
    let mut in_fence = false;
    let mut words = 0usize;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("\x60\x60\x60") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence
            || trimmed.starts_with('#')
            || trimmed.starts_with('-')
            || trimmed.starts_with('*')
            || trimmed.starts_with('>')
            || trimmed.starts_with('|')
            || trimmed.is_empty()
        {
            continue;
        }
        words += trimmed
            .split_whitespace()
            .map(|word| word.trim_matches(|ch: char| !ch.is_alphanumeric() && ch != '_'))
            .filter(|word| !word.is_empty())
            .count();
    }
    words
}

fn contains_markdown_link(content: &str, page: &str) -> bool {
    content.contains(&format!("]({page})"))
        || content.contains(&format!("](./{page})"))
        || content.contains(&format!("]({page}#"))
        || content.contains(&format!("](./{page}#"))
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

/// Assess the final tree rather than merely checking that its IDs are known.
///
/// A flat tree can cover every selected component and still be unusable as a
/// wiki.  The reference workflow treats a module as a leaf only when its
/// clustering input fits the configured limits; this side-channel metric
/// gives the host agent the same invariant without making the Rust CLI call an
/// LLM itself.
fn assess_tree_quality(
    state: &SessionState,
    tree: &ModuleTree,
    nodes: &BTreeMap<String, Node>,
    candidate_ids: &BTreeSet<String>,
) -> Value {
    let summary: Summary =
        session::read_json(&session::session_value_path(state, "summary.json")).unwrap_or_default();
    let module_limit = nonzero_or(summary.max_token_per_module, DEFAULT_MAX_TOKEN_PER_MODULE);
    let leaf_limit = nonzero_or(
        summary.max_token_per_leaf_module,
        DEFAULT_MAX_TOKEN_PER_LEAF_MODULE,
    );
    let batch_size = nonzero_or(summary.cluster_batch_size, DEFAULT_CLUSTER_BATCH_SIZE);
    let context = TreeQualityContext {
        nodes,
        candidate_ids,
        module_limit,
        max_depth: summary.max_depth,
    };

    let mut metrics = TreeQualityMetrics::default();
    for (name, module) in tree {
        collect_tree_quality(name, module, &[], 1, &context, None, &mut metrics);
    }

    let orphaned = candidate_ids
        .difference(&metrics.leaf_candidate_ids)
        .cloned()
        .collect::<Vec<_>>();
    let mut quality_errors = Vec::new();
    if !metrics.oversized_leaf_modules.is_empty() {
        quality_errors.push(format!(
            "{} leaf module(s) exceed recursive clustering limits",
            metrics.oversized_leaf_modules.len()
        ));
    }
    if !orphaned.is_empty() {
        quality_errors.push(format!(
            "{} analysis candidate(s) are not owned by a final leaf module",
            orphaned.len()
        ));
    }
    quality_errors.extend(metrics.relationship_errors.iter().cloned());
    if !metrics.depth_errors.is_empty() {
        quality_errors.extend(metrics.depth_errors.iter().cloned());
    }

    json!({
        "quality_valid": quality_errors.is_empty(),
        "quality_errors": quality_errors,
        "module_count": metrics.module_count,
        "leaf_count": metrics.leaf_count,
        "max_depth": metrics.max_depth,
        "oversized_leaf_modules": metrics.oversized_leaf_modules,
        "oversized_leaf_warnings": metrics.oversized_leaf_warnings,
        "orphaned_candidate_ids": orphaned,
        "tree_relationship_errors": metrics.relationship_errors,
        "depth_errors": metrics.depth_errors,
        "limits": {
            "max_token_per_module": module_limit,
            "max_token_per_leaf_module": leaf_limit,
            "cluster_batch_size": batch_size,
            "max_depth": summary.max_depth,
        },
    })
}

#[derive(Default)]
struct TreeQualityMetrics {
    module_count: usize,
    leaf_count: usize,
    max_depth: usize,
    leaf_candidate_ids: BTreeSet<String>,
    leaf_owners: BTreeMap<String, String>,
    oversized_leaf_modules: Vec<Value>,
    oversized_leaf_warnings: Vec<Value>,
    relationship_errors: Vec<String>,
    depth_errors: Vec<String>,
}

struct TreeQualityContext<'a> {
    nodes: &'a BTreeMap<String, Node>,
    candidate_ids: &'a BTreeSet<String>,
    module_limit: usize,
    max_depth: usize,
}

fn collect_tree_quality(
    name: &str,
    module: &Module,
    parent_path: &[String],
    depth: usize,
    context: &TreeQualityContext<'_>,
    parent_candidate_ids: Option<&BTreeSet<String>>,
    metrics: &mut TreeQualityMetrics,
) -> BTreeSet<String> {
    let mut path = parent_path.to_vec();
    path.push(name.to_string());
    metrics.module_count += 1;
    metrics.max_depth = metrics.max_depth.max(depth);
    if context.max_depth > 0 && depth > context.max_depth {
        metrics.depth_errors.push(format!(
            "module '{}' exceeds configured max depth {}",
            path.join("/"),
            context.max_depth
        ));
    }

    let own_candidate_ids = module
        .components
        .iter()
        .filter(|id| context.candidate_ids.contains(*id))
        .cloned()
        .collect::<BTreeSet<_>>();
    if let Some(parent_candidate_ids) = parent_candidate_ids {
        let missing = own_candidate_ids
            .difference(parent_candidate_ids)
            .cloned()
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            metrics.relationship_errors.push(format!(
                "module '{name}' contains candidate IDs absent from its parent: {}",
                missing.join(", ")
            ));
        }
    }

    if module.children.is_empty() {
        metrics.leaf_count += 1;
        let leaf_ids = own_candidate_ids;
        for id in &leaf_ids {
            if let Some(previous) = metrics.leaf_owners.insert(id.clone(), path.join("/")) {
                if previous != path.join("/") {
                    metrics.relationship_errors.push(format!(
                        "candidate '{}' is assigned to multiple leaf modules: '{}' and '{}'",
                        id,
                        previous,
                        path.join("/")
                    ));
                }
            }
            metrics.leaf_candidate_ids.insert(id.clone());
        }
        let estimated_tokens = leaf_ids
            .iter()
            .filter_map(|id| context.nodes.get(id))
            .map(|node| estimate_tokens(&node.source_code))
            .fold(0usize, usize::saturating_add);
        if estimated_tokens > context.module_limit && leaf_ids.len() > 1 {
            metrics.oversized_leaf_modules.push(json!({
                "module": name,
                "path": path,
                "component_count": leaf_ids.len(),
                "estimated_tokens": estimated_tokens,
                "max_token_per_module": context.module_limit,
            }));
        } else if estimated_tokens > context.module_limit {
            // A single component cannot be semantically split by the module
            // clustering pass. Keep it documentable, but make the trade-off
            // visible to the host instead of failing session close.
            metrics.oversized_leaf_warnings.push(json!({
                "module": name,
                "path": path,
                "component_count": leaf_ids.len(),
                "estimated_tokens": estimated_tokens,
                "max_token_per_module": context.module_limit,
                "reason": "singleton component cannot be recursively partitioned",
            }));
        }
        return leaf_ids;
    }

    let mut descendant_candidate_ids = BTreeSet::new();
    for (child_name, child) in &module.children {
        descendant_candidate_ids.extend(collect_tree_quality(
            child_name,
            child,
            &path,
            depth + 1,
            context,
            Some(&own_candidate_ids),
            metrics,
        ));
    }
    let missing_from_parent = descendant_candidate_ids
        .difference(&own_candidate_ids)
        .cloned()
        .collect::<Vec<_>>();
    if !missing_from_parent.is_empty() {
        metrics.relationship_errors.push(format!(
            "module '{name}' does not aggregate all descendant candidate IDs: {}",
            missing_from_parent.join(", ")
        ));
    }
    descendant_candidate_ids
}

fn estimate_tokens(source: &str) -> usize {
    let characters = source.chars().count();
    characters.saturating_add(3) / 4
}

fn nonzero_or(value: usize, fallback: usize) -> usize {
    if value == 0 {
        fallback
    } else {
        value
    }
}

fn collect_metadata(
    tree: &ModuleTree,
    output: &Path,
    depth: usize,
    count: &mut usize,
    max_depth: &mut usize,
    leaf_count: &mut usize,
    files: &mut Vec<String>,
) {
    for (name, module) in tree {
        *count += 1;
        *max_depth = (*max_depth).max(depth);
        if module.children.is_empty() {
            *leaf_count += 1;
        }
        let file = module_page_filename(name);
        if output.join(&file).exists() && !files.iter().any(|item| item == &file) {
            files.push(file);
        }
        collect_metadata(
            &module.children,
            output,
            depth + 1,
            count,
            max_depth,
            leaf_count,
            files,
        );
    }
}

fn collect_expected_pages(tree: &ModuleTree, expected: &mut BTreeSet<String>) {
    for (name, module) in tree {
        expected.insert(module_page_filename(name));
        collect_expected_pages(&module.children, expected);
    }
}

fn document_path(state: &SessionState, requested: &str) -> Result<PathBuf> {
    let requested = requested
        .strip_prefix(".repowiki/")
        .or_else(|| requested.strip_prefix("docs/"))
        .unwrap_or(requested);
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
