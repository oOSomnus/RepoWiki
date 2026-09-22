use crate::model::{
    DecompositionDecision, DecompositionReview, Metadata, Module, ModuleTree, Node, Statistics,
    Summary, DEFAULT_CLUSTER_BATCH_SIZE, DEFAULT_MAX_TOKEN_PER_LEAF_MODULE,
    DEFAULT_MAX_TOKEN_PER_MODULE,
};
use crate::session::{self, SessionState};
use anyhow::{anyhow, Context, Result};
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

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
    pub reused: bool,
    pub mermaid: MermaidReport,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MermaidReport {
    pub blocks: usize,
    pub balanced: bool,
    pub validator: String,
    #[serde(default)]
    pub diagram_kind: String,
    #[serde(default)]
    pub node_count: usize,
    #[serde(default)]
    pub edge_count: usize,
    #[serde(default)]
    pub grounded_node_count: usize,
    #[serde(default)]
    pub grounded_edge_count: usize,
    #[serde(default)]
    pub generic_node_count: usize,
    #[serde(default)]
    pub disconnected_node_count: usize,
    #[serde(default)]
    pub has_primary_path: bool,
    #[serde(default)]
    pub architecture_quality: String,
    #[serde(default)]
    pub quality_issues: Vec<String>,
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
    pub decomposition_review_valid: bool,
    pub decomposition_review_errors: Vec<String>,
    pub decomposition_review_warnings: Vec<Value>,
    pub unmatched_architecture_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ProcessingItem {
    pub module: String,
    pub doc_path: String,
    pub path: Vec<String>,
    pub is_leaf: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<String>,
    pub components: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProcessingItemInput {
    module: String,
    doc_path: String,
    path: Vec<String>,
    is_leaf: bool,
    #[serde(default)]
    children: Vec<String>,
    components: Vec<String>,
}

impl<'de> Deserialize<'de> for ProcessingItem {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let input = ProcessingItemInput::deserialize(deserializer)?;
        let canonical_doc_path = module_page_filename(&input.module)
            .map_err(|error| serde::de::Error::custom(error.to_string()))?;
        if input.doc_path != canonical_doc_path {
            return Err(serde::de::Error::custom(format!(
                "processing item doc_path must be '{canonical_doc_path}', got '{}'",
                input.doc_path
            )));
        }
        Ok(Self {
            module: input.module,
            doc_path: input.doc_path,
            path: input.path,
            is_leaf: input.is_leaf,
            children: input.children,
            components: input.components,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum EditOperation {
    #[serde(rename = "str_replace")]
    StrReplace { old: String, new: String },
    #[serde(rename = "insert")]
    Insert { line: usize, text: String },
    #[serde(rename = "undo")]
    Undo,
}

pub fn write_document(
    state: &mut SessionState,
    requested: &str,
    content: &str,
) -> Result<WriteResult> {
    write_document_with_policy(state, requested, content, false)
}

pub fn write_document_with_policy(
    state: &mut SessionState,
    requested: &str,
    content: &str,
    reuse_if_same: bool,
) -> Result<WriteResult> {
    let path = document_path(state, requested)?;
    if path.exists() {
        if reuse_if_same && session::read_text(&path)? == content {
            return Ok(WriteResult {
                path: path.to_string_lossy().into_owned(),
                created: false,
                reused: true,
                mermaid: validate_mermaid(content),
            });
        }
        if reuse_if_same {
            return Err(anyhow!(
                "document already exists with different content: {}",
                path.display()
            ));
        }
        return Err(anyhow!("document already exists: {}", path.display()));
    }
    let mermaid = validate_mermaid(content);
    session::write_text(&path, content)?;
    state.mark_write();
    session::save_state(state)?;
    Ok(WriteResult {
        path: path.to_string_lossy().into_owned(),
        created: true,
        reused: false,
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
        match operation {
            EditOperation::StrReplace { old, new } => {
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
            EditOperation::Insert { line, text } => {
                push_pending_history(&mut history_stack, &mut pending_history, &content);
                let mut lines: Vec<String> = content.split('\n').map(str::to_string).collect();
                let insert_line = (*line).min(lines.len());
                let inserted = text.split('\n').map(str::to_string).collect::<Vec<_>>();
                lines.splice(insert_line..insert_line, inserted);
                content = lines.join("\n");
            }
            EditOperation::Undo => {
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
        reused: false,
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
    save_module_tree_with_review(state, tree, first, false)
}

pub fn save_module_tree_with_review(
    state: &SessionState,
    tree: &ModuleTree,
    first: bool,
    require_decomposition_review: bool,
) -> Result<TreeSaveResult> {
    validate_module_page_paths(tree)?;
    let decomposition_review = assess_decomposition_reviews(tree, require_decomposition_review);
    let decomposition_review_errors = decomposition_review["errors"]
        .as_array()
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if require_decomposition_review && !decomposition_review_errors.is_empty() {
        return Err(anyhow!(
            "module decomposition review is incomplete: {}",
            decomposition_review_errors.join("; ")
        ));
    }
    let decomposition_review_warnings = decomposition_review["warnings"]
        .as_array()
        .cloned()
        .unwrap_or_default();
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
        )?;
    }
    let known_ids = nodes.keys().cloned().collect::<BTreeSet<_>>();
    let candidate_ids =
        session::read_json::<Vec<String>>(&session::session_value_path(state, "leaf_nodes.json"))?
            .into_iter()
            .collect::<BTreeSet<_>>();
    let unmatched = assigned.difference(&known_ids).cloned().collect::<Vec<_>>();
    let architecture_ids = collect_tree_ids(tree);
    let omitted_candidates = candidate_ids
        .difference(&architecture_ids)
        .cloned()
        .collect::<Vec<_>>();
    let quality = assess_tree_quality(state, tree, &nodes, &architecture_ids);
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
    let quality_valid = quality["quality_valid"].as_bool().unwrap_or(false);
    let validation = json!({
        "valid": unmatched.is_empty(),
        "complete": unmatched.is_empty() && quality_valid,
        "unmatched_architecture_ids": unmatched.clone(),
        "unmatched_count": unmatched.len(),
        "omitted_analysis_candidate_ids": omitted_candidates,
        "architecture_anchor_count": architecture_ids.len(),
        "module_count": module_count,
        "leaf_count": leaf_count,
        "max_depth": quality["max_depth"],
        "quality_valid": quality_valid,
        "quality_errors": quality_errors,
        "decomposition_review": decomposition_review,
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
        decomposition_review_valid: decomposition_review["valid"].as_bool().unwrap_or(false),
        decomposition_review_errors,
        decomposition_review_warnings,
        unmatched_architecture_ids: validation["unmatched_architecture_ids"]
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

/// Apply one host-agent architecture selection response to a working tree.
///
/// The architecture tree is intentionally lossy: the host selects a small
/// set of exact source anchors and the remaining analyzed components stay in
/// the dependency graph.  The CLI validates selected IDs but never invents a
/// fallback module or turns an omitted implementation detail into a page.
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
    let module_review = if scope == "module" {
        parse_decomposition_review(response)?
    } else {
        None
    };
    let parsed = parse_grouped_components(response, &mut diagnostics);
    let mut groups = Vec::<(String, Module)>::new();
    let mut claimed = BTreeSet::new();
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

    if scope == "module" && groups.is_empty() {
        let Some(review) = module_review.as_ref() else {
            return Err(anyhow!(
                "module clustering returned no child modules or decomposition review: {}",
                diagnostics.join("; ")
            ));
        };
        if review.decision != DecompositionDecision::RetainLeaf {
            return Err(anyhow!(
                "module clustering returned no child modules but decision was not retain_leaf"
            ));
        }
        let parent = module_at_path_mut(tree, parent_path)
            .ok_or_else(|| anyhow!("module path not found: {}", parent_path.join("/")))?;
        parent.decomposition_review = Some(review.clone());
        return Ok(json!({
            "scope": scope,
            "parent_path": parent_path,
            "decision": "retain_leaf",
            "input_count": input_ids.len(),
            "selected_count": 0,
            "omitted_count": 0,
            "group_count": 0,
            "diagnostics": diagnostics,
        }));
    }

    if groups.is_empty() {
        return Err(anyhow!(
            "architecture clustering returned no module anchors: {}",
            diagnostics.join("; ")
        ));
    }

    if scope == "module" {
        if module_review
            .as_ref()
            .is_some_and(|review| review.decision != DecompositionDecision::Split)
        {
            return Err(anyhow!(
                "module clustering returned child modules but decision was not split"
            ));
        }
        if let Some(review) = module_review.as_ref() {
            module_at_path_mut(tree, parent_path)
                .ok_or_else(|| anyhow!("module path not found: {}", parent_path.join("/")))?
                .decomposition_review = Some(review.clone());
        }
    }

    let omitted_count = requested.len().saturating_sub(claimed.len());
    if omitted_count > 0 {
        diagnostics.push(format!(
            "{} analyzed component(s) intentionally remain outside the architecture tree",
            omitted_count
        ));
    }

    if scope == "repo" {
        merge_modules(tree, groups);
    } else {
        let parent = module_at_path_mut(tree, parent_path)
            .ok_or_else(|| anyhow!("module path not found: {}", parent_path.join("/")))?;
        for id in &claimed {
            if !parent.components.iter().any(|component| component == id) {
                parent.components.push(id.clone());
            }
        }
        merge_modules(&mut parent.children, groups);
        for depth in 1..parent_path.len() {
            if let Some(ancestor) = module_at_path_mut(tree, &parent_path[..depth]) {
                for id in &claimed {
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
        "selected_count": claimed.len(),
        "omitted_count": omitted_count,
        "group_count": if scope == "repo" { tree.len() } else { module_at_path(tree, parent_path).map(|module| module.children.len()).unwrap_or_default() },
        "diagnostics": diagnostics,
    }))
}

/// Apply a super-group response by making the existing top-level modules
/// children of newly named architectural parents.  Existing module pages and
/// aggregate IDs are preserved verbatim.
pub fn apply_super_group_response(tree: &mut ModuleTree, response: &str) -> Result<Value> {
    let mut diagnostics = Vec::new();
    let grouping = parse_grouped_modules(response, &mut diagnostics)
        .ok_or_else(|| anyhow!("invalid super-group response: {}", diagnostics.join("; ")))?;
    if grouping.is_empty() {
        return Err(anyhow!(
            "invalid super-group response: no module groups were returned"
        ));
    }
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
                decomposition_review: info.decomposition_review,
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
    validate_module_page_paths(tree)?;
    fn visit(
        modules: &ModuleTree,
        target_path: &[String],
        prefix: &[String],
        output_dir: &Path,
    ) -> Result<Value> {
        let mut result = serde_json::Map::new();
        let target_is_here = prefix == target_path;
        for (name, module) in modules {
            let doc_path = module_page_filename(name)?;
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
            object.insert("doc_path".to_string(), Value::String(doc_path.clone()));
            object.insert(
                "is_target_for_overview_generation".to_string(),
                json!(is_target),
            );
            let children = visit(&module.children, target_path, &current, output_dir)?;
            if let Some(children_object) = children.as_object() {
                object.insert(
                    "children".to_string(),
                    Value::Object(children_object.clone()),
                );
            }
            if target_is_here {
                let docs_path = output_dir.join(&doc_path);
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
        Ok(Value::Object(result))
    }

    visit(tree, target_path, &[], output_dir)
}

/// Build the overview context used by architecture prompts.  The structural
/// tree alone is not enough to draw a useful system diagram: it tells the
/// model what pages exist, but not which modules exchange work.  This helper
/// folds component-level dependency targets into the selected architecture
/// modules and returns a small, source-backed graph for the host model.
pub fn overview_context_for_session(
    state: &SessionState,
    tree: &ModuleTree,
    target_path: &[String],
    output_dir: &Path,
) -> Result<Value> {
    let structure = overview_context(tree, target_path, output_dir)?;
    let nodes: BTreeMap<String, Node> =
        session::read_json(&session::session_value_path(state, "components.json"))?;
    Ok(json!({
        "repo_structure": structure,
        "architecture_context": build_architecture_context(tree, target_path, &nodes),
    }))
}

fn build_architecture_context(
    tree: &ModuleTree,
    target_path: &[String],
    nodes: &BTreeMap<String, Node>,
) -> Value {
    let mut component_modules = BTreeMap::<String, String>::new();
    let mut module_paths = Vec::<Vec<String>>::new();
    let mut module_descriptors = Vec::<(String, Option<String>, usize)>::new();

    fn collect_modules(
        modules: &ModuleTree,
        prefix: &[String],
        component_modules: &mut BTreeMap<String, String>,
        module_paths: &mut Vec<Vec<String>>,
        module_descriptors: &mut Vec<(String, Option<String>, usize)>,
    ) {
        for (name, module) in modules {
            let mut path = prefix.to_vec();
            path.push(name.clone());
            let label = path.join(" / ");
            module_paths.push(path.clone());
            module_descriptors.push((label.clone(), module.path.clone(), path.len()));
            for id in &module.components {
                // A child is a more precise architecture anchor than an
                // aggregate parent, so later traversal deliberately wins.
                component_modules.insert(id.clone(), label.clone());
            }
            collect_modules(
                &module.children,
                &path,
                component_modules,
                module_paths,
                module_descriptors,
            );
        }
    }

    collect_modules(
        tree,
        &[],
        &mut component_modules,
        &mut module_paths,
        &mut module_descriptors,
    );

    let root_module = module_descriptors
        .iter()
        .find(|(_, _, depth)| *depth == 1)
        .map(|(label, _, _)| label.clone());
    let source_owner = |relative_path: &str| {
        let normalized = relative_path.trim_start_matches("./");
        module_descriptors
            .iter()
            .filter_map(|(label, module_path, depth)| {
                let module_path = module_path.as_deref()?.trim_matches('/');
                if module_path.is_empty() || module_path == "." {
                    return None;
                }
                let matches = normalized == module_path
                    || normalized.starts_with(&format!("{module_path}/"))
                    || normalized.starts_with(&format!("{module_path}."));
                matches.then_some((module_path.len(), *depth, label.clone()))
            })
            .max_by_key(|(path_len, depth, _)| (*path_len, *depth))
            .map(|(_, _, label)| label)
            .or_else(|| root_module.clone())
    };

    // Selected IDs are the strongest ownership evidence.  For every other
    // analyzed node, use the module's declared source boundary so dependency
    // edges are aggregated at the architecture level instead of disappearing
    // merely because the node was not chosen as a page anchor.
    for node in nodes.values() {
        component_modules
            .entry(node.id.clone())
            .or_insert_with(|| source_owner(&node.relative_path).unwrap_or_default());
    }

    let mut module_nodes = BTreeMap::<String, Value>::new();
    for (module, _, _) in &module_descriptors {
        module_nodes.insert(
            module.clone(),
            json!({
                "id": module,
                "label": module.rsplit(" / ").next().unwrap_or(module),
                "role": architecture_role(module),
                "evidence": Vec::<String>::new(),
            }),
        );
    }
    let mut edge_counts = BTreeMap::<(String, String), (usize, Vec<String>)>::new();
    for node in nodes.values() {
        let Some(module) = component_modules
            .get(&node.id)
            .filter(|module| !module.is_empty())
        else {
            continue;
        };
        let entry = module_nodes.entry(module.clone()).or_insert_with(|| {
            json!({
                "id": module,
                "label": module.rsplit(" / ").next().unwrap_or(module),
                "role": architecture_role(module),
                "evidence": Vec::<String>::new(),
            })
        });
        if let Some(evidence) = entry.get_mut("evidence").and_then(Value::as_array_mut) {
            if evidence.len() < 8 {
                evidence.push(Value::String(node.relative_path.clone()));
            }
        }
        for dependency in &node.depends_on {
            let Some(target) = component_modules.get(dependency) else {
                continue;
            };
            if target == module {
                continue;
            }
            let edge = edge_counts
                .entry((module.clone(), target.clone()))
                .or_insert_with(|| (0, Vec::new()));
            edge.0 += 1;
            if edge.1.len() < 4 {
                edge.1.push(format!("{} -> {}", node.id, dependency));
            }
        }
    }

    // Keep the context compact and architecture-shaped.  A target module
    // includes itself and its descendants, plus any connected external module
    // that explains an incoming or outgoing edge.
    let target_prefix = target_path.join(" / ");
    let mut selected = BTreeSet::new();
    if target_prefix.is_empty() {
        selected.extend(module_nodes.keys().cloned());
    } else {
        for module in module_nodes.keys() {
            if module == &target_prefix || module.starts_with(&(target_prefix.clone() + " / ")) {
                selected.insert(module.clone());
            }
        }
        for (from, to) in edge_counts.keys() {
            if selected.contains(from) || selected.contains(to) {
                selected.insert(from.clone());
                selected.insert(to.clone());
            }
        }
    }

    let mut nodes_json = Vec::new();
    for module in &selected {
        if let Some(node) = module_nodes.get(module) {
            nodes_json.push(node.clone());
        }
    }
    let mut edges_json = Vec::new();
    for ((from, to), (count, evidence)) in edge_counts {
        if selected.contains(&from) && selected.contains(&to) {
            edges_json.push(json!({
                "from": from,
                "to": to,
                "relation": "dependency",
                "weight": count,
                "evidence": evidence,
            }));
        }
    }

    let mut adjacency = BTreeMap::<String, Vec<String>>::new();
    let mut incoming = BTreeMap::<String, usize>::new();
    for edge in &edges_json {
        let from = edge["from"].as_str().unwrap_or_default().to_string();
        let to = edge["to"].as_str().unwrap_or_default().to_string();
        adjacency.entry(from).or_default().push(to.clone());
        *incoming.entry(to).or_default() += 1;
    }
    for targets in adjacency.values_mut() {
        targets.sort();
    }
    let mut starts = selected
        .iter()
        .filter(|module| !incoming.contains_key(*module))
        .cloned()
        .collect::<Vec<_>>();
    starts.sort();
    let mut primary_path = Vec::new();
    if let Some(mut current) = starts.first().cloned() {
        let mut seen = BTreeSet::new();
        loop {
            if !seen.insert(current.clone()) {
                break;
            }
            primary_path.push(current.clone());
            let next = adjacency
                .get(&current)
                .and_then(|targets| targets.iter().find(|target| !seen.contains(*target)))
                .cloned();
            let Some(next) = next else { break };
            current = next;
        }
    }
    if primary_path.len() < 2 {
        primary_path = module_paths
            .iter()
            .filter(|path| target_path.is_empty() || path.starts_with(target_path))
            .map(|path| path.join(" / "))
            .take(8)
            .collect();
    }

    json!({
        "target": if target_path.is_empty() { "repository".to_string() } else { target_prefix },
        "nodes": nodes_json,
        "edges": edges_json,
        "primary_paths": if primary_path.is_empty() { Vec::<Vec<String>>::new() } else { vec![primary_path] },
        "note": "Use this graph as grounded architecture evidence. It is not a source index and it is not a required diagram layout.",
    })
}

fn architecture_role(module: &str) -> &'static str {
    let lower = module.to_ascii_lowercase();
    if lower.contains("storage") || lower.contains("database") || lower.contains("state") {
        "state"
    } else if lower.contains("protocol")
        || lower.contains("api")
        || lower.contains("client")
        || lower.contains("server")
    {
        "interface"
    } else if lower.contains("execution")
        || lower.contains("runtime")
        || lower.contains("pipeline")
        || lower.contains("interpreter")
    {
        "execution"
    } else if lower.contains("build")
        || lower.contains("test")
        || lower.contains("release")
        || lower.contains("packag")
    {
        "support"
    } else if lower.contains("network") || lower.contains("integration") {
        "integration"
    } else {
        "subsystem"
    }
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

fn parse_decomposition_review(response: &str) -> Result<Option<DecompositionReview>> {
    const START: &str = "<DECOMPOSITION_REVIEW>";
    const END: &str = "</DECOMPOSITION_REVIEW>";
    let Some(start) = response.find(START) else {
        return Ok(None);
    };
    let body_start = start + START.len();
    let body_end = response[body_start..]
        .find(END)
        .map(|offset| body_start + offset)
        .ok_or_else(|| anyhow!("missing </DECOMPOSITION_REVIEW> marker"))?;
    let body = response[body_start..body_end]
        .trim()
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    serde_json::from_str(body)
        .context("invalid DECOMPOSITION_REVIEW JSON")
        .map(Some)
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
    let decomposition_review = info
        .get("decomposition_review")
        .cloned()
        .map(serde_json::from_value::<DecompositionReview>)
        .transpose()
        .unwrap_or_else(|error| {
            diagnostics.push(format!("invalid decomposition_review: {error}"));
            None
        });
    Module {
        path,
        components,
        children,
        decomposition_review,
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
            if incoming.decomposition_review.is_some() {
                existing.decomposition_review = incoming.decomposition_review.take();
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

fn collect_tree_ids(tree: &ModuleTree) -> BTreeSet<String> {
    fn visit(modules: &ModuleTree, ids: &mut BTreeSet<String>) {
        for module in modules.values() {
            ids.extend(module.components.iter().cloned());
            visit(&module.children, ids);
        }
    }
    let mut ids = BTreeSet::new();
    visit(tree, &mut ids);
    ids
}

fn assess_decomposition_reviews(tree: &ModuleTree, required: bool) -> Value {
    fn visit(
        modules: &ModuleTree,
        parent_path: &[String],
        required: bool,
        reviewed: &mut usize,
        missing: &mut Vec<Value>,
        errors: &mut Vec<String>,
        warnings: &mut Vec<Value>,
    ) {
        use crate::model::{BreadthRisk, DecompositionDecision};

        for (name, module) in modules {
            let mut path = parent_path.to_vec();
            path.push(name.clone());
            let path_label = path.join("/");
            match &module.decomposition_review {
                Some(review) => {
                    *reviewed += 1;
                    if review.reason.trim().is_empty() {
                        if required {
                            errors.push(format!(
                                "module '{path_label}' has an empty decomposition reason"
                            ));
                        }
                    }
                    let expected = if module.children.is_empty() {
                        DecompositionDecision::RetainLeaf
                    } else {
                        DecompositionDecision::Split
                    };
                    if review.decision != expected && required {
                        errors.push(format!(
                            "module '{path_label}' decomposition decision does not match its children"
                        ));
                    }
                    if module.children.is_empty() && review.breadth_risk == BreadthRisk::High {
                        warnings.push(json!({
                            "module": name,
                            "path": path,
                            "reason": review.reason,
                            "warning": "high breadth risk was retained as a leaf",
                        }));
                    }
                }
                None => {
                    if required {
                        errors.push(format!("module '{path_label}' has no decomposition review"));
                    }
                    missing.push(json!({"module": name, "path": path}));
                }
            }
            visit(
                &module.children,
                &path,
                required,
                reviewed,
                missing,
                errors,
                warnings,
            );
        }
    }

    let mut reviewed = 0;
    let mut missing = Vec::new();
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    visit(
        tree,
        &[],
        required,
        &mut reviewed,
        &mut missing,
        &mut errors,
        &mut warnings,
    );
    json!({
        "required": required,
        "valid": errors.is_empty(),
        "reviewed_module_count": reviewed,
        "missing": missing,
        "errors": errors,
        "warnings": warnings,
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
    validate_module_page_paths(&tree)?;
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
    )?;
    let architecture_anchors = collect_tree_ids(&tree).len();
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
            architecture_modules: count,
            architecture_anchors,
        },
        files_generated,
        documentation_profile: "architecture".to_string(),
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
    for field in ["unmatched_architecture_ids"] {
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
    if validation["decomposition_review"]["required"] == Value::Bool(true)
        && validation["decomposition_review"]["valid"] == Value::Bool(false)
    {
        let errors = validation["decomposition_review"]["errors"]
            .as_array()
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("; ")
            })
            .unwrap_or_default();
        return Err(anyhow!(
            "incomplete documentation: module decomposition review failed: {errors}"
        ));
    }

    let tree: ModuleTree = session::read_json(&output.join("module_tree.json"))?;
    validate_module_page_paths(&tree)?;
    let mut expected = BTreeSet::new();
    collect_expected_pages(&tree, &mut expected)?;
    expected.insert("overview.md".to_string());
    let missing = expected
        .iter()
        .filter(|page| !output.join(page).is_file())
        .cloned()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(anyhow!(
            "incomplete documentation: missing module pages {}",
            missing.join(", ")
        ));
    }

    let extra_pages = top_level_markdown_pages(&output)?
        .into_iter()
        .filter(|page| !expected.contains(page))
        .collect::<Vec<_>>();
    let mut broken_links = Vec::new();
    for page in &expected {
        let content = fs::read_to_string(output.join(page))?;
        for target in markdown_link_targets(&content) {
            if expected.contains(&target) {
                continue;
            }
            broken_links.push(json!({"page": page, "target": target}));
        }
    }

    let nodes: BTreeMap<String, Node> =
        session::read_json(&session::session_value_path(state, "components.json"))?;
    let mut builder = DocumentationReportBuilder::new(&output, &nodes);
    builder.visit_modules(&tree)?;
    let overview_path = output.join("overview.md");
    let overview = fs::read_to_string(&overview_path)?;
    builder.visit_overview(&tree, &overview)?;
    let DocumentationReportBuilder {
        pages,
        errors: builder_errors,
        page_count,
        valid_page_count,
        explanatory_page_count,
        mermaid_page_count,
        grounded_page_count,
        ..
    } = builder;
    let mut errors = builder_errors;
    let decomposition_review_warnings = validation["decomposition_review"]["warnings"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    errors.extend(
        extra_pages
            .iter()
            .map(|page| format!("unexpected Markdown page: {page}")),
    );
    errors.extend(broken_links.iter().filter_map(|link| {
        Some(format!(
            "broken Markdown link in {}: {}",
            link["page"].as_str()?,
            link["target"].as_str()?
        ))
    }));

    let report = json!({
        "valid": errors.is_empty(),
        "errors": errors,
        "extra_pages": extra_pages,
        "broken_links": broken_links,
        "warnings": decomposition_review_warnings,
        "prose_count_mode": "language-aware-v2",
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
            "canonical_page_paths": true,
            "module_decomposition_review": validation["decomposition_review"]["required"]
                == Value::Bool(true),
            "no_extra_markdown_pages": true,
            "no_broken_markdown_links": true,
        },
    });
    session::write_json(
        &session::session_value_path(state, "documentation_validation.json"),
        &report,
    )?;
    Ok(report)
}

pub fn validate_mermaid(content: &str) -> MermaidReport {
    validate_mermaid_with_context(content, &[], false, false)
}

fn validate_mermaid_with_context(
    content: &str,
    grounded_labels: &[String],
    strict: bool,
    forbid_support_nodes: bool,
) -> MermaidReport {
    let blocks = extract_mermaid_blocks(content);
    let balanced = content
        .lines()
        .filter(|line| line.trim().to_ascii_lowercase().starts_with("```mermaid"))
        .count()
        == blocks.len()
        && !content
            .lines()
            .scan(false, |open, line| {
                let trimmed = line.trim().to_ascii_lowercase();
                if trimmed.starts_with("```mermaid") {
                    *open = true;
                } else if *open && trimmed.starts_with("```") {
                    *open = false;
                }
                Some(*open)
            })
            .last()
            .unwrap_or(false);

    let mut kinds = BTreeSet::new();
    let mut nodes = BTreeMap::<String, String>::new();
    let mut edges = Vec::<(String, String)>::new();
    for block in &blocks {
        let stats = parse_mermaid_block(block);
        if !stats.kind.is_empty() {
            kinds.insert(stats.kind);
        }
        nodes.extend(stats.nodes);
        edges.extend(stats.edges);
    }

    let normalized_grounded = grounded_labels
        .iter()
        .map(|label| normalize_diagram_text(label))
        .filter(|label| !label.is_empty())
        .collect::<Vec<_>>();
    let grounded_node_count = if normalized_grounded.is_empty() {
        0
    } else {
        nodes
            .values()
            .filter(|label| {
                let value = normalize_diagram_text(label);
                normalized_grounded
                    .iter()
                    .any(|allowed| value.contains(allowed) || allowed.contains(&value))
            })
            .count()
    };
    let grounded_edge_count = if normalized_grounded.is_empty() {
        0
    } else {
        edges
            .iter()
            .filter(|(from, to)| {
                let from = normalize_diagram_text(nodes.get(from).unwrap_or(from));
                let to = normalize_diagram_text(nodes.get(to).unwrap_or(to));
                normalized_grounded.iter().any(|allowed| {
                    from.contains(allowed)
                        || to.contains(allowed)
                        || allowed.contains(&from)
                        || allowed.contains(&to)
                })
            })
            .count()
    };
    let generic_node_count = nodes
        .iter()
        .filter(|(id, label)| is_generic_diagram_label(id) || is_generic_diagram_label(label))
        .count();
    let disconnected_node_count = nodes
        .keys()
        .filter(|node| !edges.iter().any(|(from, to)| from == *node || to == *node))
        .count();
    // A tiny repository can have a legitimate two-stage architecture.  Keep
    // the architecture gate strict for real overviews, but do not reject a
    // grounded two-node system merely because it cannot contain three stages.
    let minimum_path_edges = if strict && forbid_support_nodes && nodes.len() > 2 {
        3
    } else if nodes.len() > 1 {
        1
    } else {
        0
    };
    let has_primary_path = has_diagram_path(&nodes, &edges, minimum_path_edges);
    let support_nodes = nodes
        .iter()
        .filter(|(id, label)| is_support_diagram_label(id) || is_support_diagram_label(label))
        .count();

    let mut quality_issues = Vec::new();
    if !balanced {
        quality_issues.push("unbalanced Mermaid fence".to_string());
    }
    if strict && blocks.is_empty() {
        quality_issues.push("architecture page has no Mermaid diagram".to_string());
    }
    let minimum_nodes: usize = if strict { 2 } else { 0 };
    let minimum_edges = minimum_nodes.saturating_sub(1);
    if strict && nodes.len() < minimum_nodes {
        quality_issues.push(format!(
            "diagram has fewer than {minimum_nodes} architecture nodes"
        ));
    }
    if strict && edges.len() < minimum_edges {
        quality_issues.push(format!(
            "diagram has fewer than {minimum_edges} architecture edge(s)"
        ));
    }
    if strict && !has_primary_path {
        quality_issues.push("diagram has no connected multi-stage path".to_string());
    }
    if strict && generic_node_count * 2 > nodes.len().max(1) {
        quality_issues.push("diagram is dominated by generic placeholder nodes".to_string());
    }
    if strict && !normalized_grounded.is_empty() {
        let required = if nodes.len() <= 2 {
            1
        } else {
            (nodes.len() / 2).max(2)
        };
        if grounded_node_count < required {
            quality_issues.push(format!(
                "only {grounded_node_count} of {} diagram nodes match architecture context",
                nodes.len()
            ));
        }
    }
    if forbid_support_nodes && support_nodes > 0 {
        quality_issues.push(
            "runtime overview diagram includes build, test, release, or packaging nodes"
                .to_string(),
        );
    }

    MermaidReport {
        blocks: blocks.len(),
        balanced,
        validator: "architecture-heuristic".to_string(),
        diagram_kind: kinds.into_iter().collect::<Vec<_>>().join(","),
        node_count: nodes.len(),
        edge_count: edges.len(),
        grounded_node_count,
        grounded_edge_count,
        generic_node_count,
        disconnected_node_count,
        has_primary_path,
        architecture_quality: if quality_issues.is_empty() {
            "pass".to_string()
        } else if strict {
            "fail".to_string()
        } else {
            "warning".to_string()
        },
        quality_issues,
    }
}

#[derive(Default)]
struct MermaidBlockStats {
    kind: String,
    nodes: BTreeMap<String, String>,
    edges: Vec<(String, String)>,
}

fn extract_mermaid_blocks(content: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current = None::<String>;
    for line in content.lines() {
        let lower = line.trim().to_ascii_lowercase();
        if lower.starts_with("```mermaid") {
            current = Some(String::new());
        } else if current.is_some() && lower.starts_with("```") {
            if let Some(block) = current.take() {
                blocks.push(block);
            }
        } else if let Some(block) = current.as_mut() {
            block.push_str(line);
            block.push('\n');
        }
    }
    blocks
}

fn parse_mermaid_block(block: &str) -> MermaidBlockStats {
    let mut result = MermaidBlockStats::default();
    for raw_line in block.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with("%%") {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("flowchart ") || lower.starts_with("graph ") {
            result.kind = line
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_string();
        } else if lower == "sequencediagram" {
            result.kind = "sequenceDiagram".to_string();
        } else if lower.starts_with("participant ") {
            let token = line.split_whitespace().nth(1).unwrap_or_default();
            add_mermaid_node(&mut result.nodes, token, token);
        }

        if lower.starts_with("subgraph ") || lower.starts_with("style ") {
            continue;
        }
        for arrow in ["-->>", "-.->", "==>", "-->", "->>", "->"] {
            let Some((left, right)) = line.split_once(arrow) else {
                continue;
            };
            let left = left.trim();
            let right = right
                .split_once(':')
                .map(|(value, _)| value)
                .unwrap_or(right)
                .trim();
            let left_id = add_mermaid_node(&mut result.nodes, left, left);
            let right_id = add_mermaid_node(&mut result.nodes, right, right);
            result.edges.push((left_id, right_id));
            break;
        }
        if result.kind != "sequenceDiagram" {
            for token in line.split_whitespace() {
                if token.contains('[') || token.contains('(') || token.contains('{') {
                    add_mermaid_node(&mut result.nodes, token, token);
                }
            }
        }
    }
    result
}

fn add_mermaid_node(nodes: &mut BTreeMap<String, String>, raw: &str, fallback: &str) -> String {
    let raw = raw.trim().trim_matches(';').trim_matches('`');
    let id_end = raw
        .find(|ch| ['[', '(', '{', '<', '|'].contains(&ch))
        .unwrap_or(raw.len());
    let id = raw[..id_end].trim().trim_matches('"').to_string();
    if id.is_empty() {
        return fallback.to_string();
    }
    let label = raw[id_end..]
        .trim_matches(|ch| matches!(ch, '[' | ']' | '(' | ')' | '{' | '}' | '"' | '`'))
        .split('|')
        .next()
        .unwrap_or_default()
        .trim()
        .to_string();
    nodes
        .entry(id.clone())
        .or_insert_with(|| if label.is_empty() { id.clone() } else { label });
    id
}

fn normalize_diagram_text(value: &str) -> String {
    value
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || ch.is_ascii_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_generic_diagram_label(label: &str) -> bool {
    matches!(
        normalize_diagram_text(label).as_str(),
        "caller"
            | "entry"
            | "core"
            | "result"
            | "reader"
            | "readers and listings"
            | "model"
            | "record"
            | "parent"
            | "child"
            | "module"
            | "component"
            | "source"
            | "target"
            | "input"
            | "output"
            | "fixture"
            | "assertion"
            | "all"
            | "other"
    )
}

fn is_support_diagram_label(label: &str) -> bool {
    let value = normalize_diagram_text(label);
    [
        "build",
        "test",
        "release",
        "smoke",
        "verify",
        "packag",
        "deployment",
        "ci",
    ]
    .iter()
    .any(|term| value.contains(term))
}

fn has_diagram_path(
    nodes: &BTreeMap<String, String>,
    edges: &[(String, String)],
    minimum_edges: usize,
) -> bool {
    if nodes.is_empty() || edges.is_empty() {
        return false;
    }
    let mut adjacency = BTreeMap::<String, Vec<String>>::new();
    for (from, to) in edges {
        adjacency.entry(from.clone()).or_default().push(to.clone());
    }
    fn visit(
        node: &str,
        adjacency: &BTreeMap<String, Vec<String>>,
        seen: &mut BTreeSet<String>,
        length: usize,
        minimum_edges: usize,
    ) -> bool {
        if length >= minimum_edges {
            return true;
        }
        if !seen.insert(node.to_string()) {
            return false;
        }
        let found = adjacency.get(node).is_some_and(|children| {
            children
                .iter()
                .any(|child| visit(child, adjacency, seen, length + 1, minimum_edges))
        });
        seen.remove(node);
        found
    }
    nodes
        .keys()
        .any(|node| visit(node, &adjacency, &mut BTreeSet::new(), 0, minimum_edges))
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

    fn visit_modules(&mut self, modules: &ModuleTree) -> Result<()> {
        for (name, module) in modules {
            let page = module_page_filename(name)?;
            let path = self.output.join(&page);
            let content = fs::read_to_string(&path).unwrap_or_default();
            let required_links = module
                .children
                .keys()
                .map(|child| module_page_filename(child))
                .collect::<Result<Vec<_>>>()?;
            let labels = module
                .children
                .keys()
                .cloned()
                .chain(std::iter::once(name.clone()))
                .collect::<Vec<_>>();
            let mut diagram_labels = labels.clone();
            collect_module_diagram_labels(name, module, self.nodes, &mut diagram_labels);
            let result = assess_page(
                name,
                module,
                &content,
                self.nodes,
                PageAssessmentContext {
                    is_leaf: module.children.is_empty(),
                    is_overview: false,
                    required_links: &required_links,
                    grounded_labels: &labels,
                    diagram_labels: &diagram_labels,
                },
            );
            self.record_page(&page, result);
            self.visit_modules(&module.children)?;
        }
        Ok(())
    }

    fn visit_overview(&mut self, tree: &ModuleTree, content: &str) -> Result<()> {
        let overview_links = tree
            .keys()
            .map(|name| module_page_filename(name))
            .collect::<Result<Vec<_>>>()?;
        let mut labels = Vec::new();
        collect_module_labels(tree, &mut labels);
        let mut diagram_labels = labels.clone();
        collect_tree_diagram_labels(tree, self.nodes, &mut diagram_labels);
        let result = assess_page(
            "Repository overview",
            &Module::default(),
            content,
            self.nodes,
            PageAssessmentContext {
                is_leaf: false,
                is_overview: true,
                required_links: &overview_links,
                grounded_labels: &labels,
                diagram_labels: &diagram_labels,
            },
        );
        self.record_page("overview.md", result);
        Ok(())
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
        if result["grounded_components"].as_u64().unwrap_or_default() > 0
            || result["grounded_modules"].as_u64().unwrap_or_default() > 0
        {
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
struct PageAssessmentContext<'a> {
    is_leaf: bool,
    is_overview: bool,
    required_links: &'a [String],
    grounded_labels: &'a [String],
    diagram_labels: &'a [String],
}

fn assess_page(
    name: &str,
    module: &Module,
    content: &str,
    nodes: &BTreeMap<String, Node>,
    context: PageAssessmentContext<'_>,
) -> Value {
    let PageAssessmentContext {
        is_leaf,
        is_overview,
        required_links,
        grounded_labels,
        diagram_labels,
    } = context;
    let headings = markdown_headings(content);
    let lower = content.to_ascii_lowercase();
    let mermaid = validate_mermaid(content);
    let prose_counts = markdown_prose_counts(content);
    let cjk_mode = prose_counts.cjk_characters > prose_counts.words;
    let prose_words = if cjk_mode {
        prose_counts.cjk_characters
    } else {
        prose_counts.words
    };
    let purpose = has_heading_term(
        &headings,
        &[
            "purpose", "scope", "role", "目的", "职责", "作用", "范围", "简介",
        ],
    );
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
            "架构",
            "设计",
            "数据流",
            "流程",
            "执行",
            "生命周期",
            "依赖",
            "组件交互",
            "行为",
            "运行",
        ],
    );
    let responsibilities = has_heading_term(
        &headings,
        &[
            "responsibilities",
            "interfaces",
            "usage",
            "error",
            "responsibility",
            "职责",
            "接口",
            "使用",
            "错误",
            "异常",
            "状态",
            "能力",
            "实现",
        ],
    );
    let semantic_sections =
        usize::from(purpose) + usize::from(architecture) + usize::from(responsibilities);
    let prose_limit = if cjk_mode {
        if is_leaf {
            500
        } else {
            700
        }
    } else if is_leaf {
        150
    } else {
        200
    };
    let explanatory = prose_words >= prose_limit && semantic_sections >= 2;

    let mut grounded_components = 0usize;
    for component_id in &module.components {
        let Some(node) = nodes.get(component_id) else {
            continue;
        };
        let source_path = if node.relative_path.is_empty() {
            node.file_path.as_str()
        } else {
            node.relative_path.as_str()
        };
        let symbol = if node.name.is_empty() {
            node.display_name.as_deref().unwrap_or_default()
        } else {
            &node.name
        };
        let exact_id = content.contains(component_id);
        let symbol_and_path = !symbol.is_empty()
            && !source_path.is_empty()
            && content
                .split("\n\n")
                .any(|paragraph| paragraph.contains(symbol) && paragraph.contains(source_path));
        if exact_id || symbol_and_path {
            grounded_components += 1;
        }
    }
    let grounded_modules = grounded_labels
        .iter()
        .filter(|label| !label.is_empty() && content.contains(label.as_str()))
        .count();

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
    if prose_words < prose_limit {
        page_errors.push(format!(
            "contains only {prose_words} explanatory {}; expected at least {prose_limit}",
            if cjk_mode {
                "CJK characters"
            } else {
                "English words"
            }
        ));
    }
    let minimum_grounded_components = module.components.len().min(2);
    if minimum_grounded_components > 0 && grounded_components < minimum_grounded_components {
        page_errors.push(format!(
            "mentions {grounded_components} analyzed component(s); expected at least {minimum_grounded_components} source anchors"
        ));
    }
    if semantic_sections < 2 {
        page_errors.push(
            "covers fewer than two semantic areas such as purpose, architecture, interfaces, behavior, or lifecycle"
                .to_string(),
        );
    }
    let diagram = validate_mermaid_with_context(content, diagram_labels, !is_leaf, is_overview);
    if diagram.architecture_quality == "fail" {
        page_errors.extend(diagram.quality_issues.iter().cloned());
    }
    if template_only {
        page_errors.push(
            "looks like a generated component-list template rather than an explanatory page"
                .to_string(),
        );
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
        "prose_count_mode": if cjk_mode { "cjk_characters" } else { "english_words" },
        "prose_floor": prose_limit,
        "semantic_sections": semantic_sections,
        "grounded_components": grounded_components,
        "grounded_modules": grounded_modules,
        "component_count": module.components.len(),
        "mermaid_blocks": mermaid.blocks,
        "mermaid_balanced": mermaid.balanced,
        "architecture_diagram": diagram,
        "missing_links": missing_links,
        "boilerplate_detected": template_only || lower.contains("this leaf documents a cohesive implementation area"),
    })
}

fn collect_module_labels(tree: &ModuleTree, labels: &mut Vec<String>) {
    for (name, module) in tree {
        labels.push(name.clone());
        collect_module_labels(&module.children, labels);
    }
}

fn collect_tree_diagram_labels(
    tree: &ModuleTree,
    nodes: &BTreeMap<String, Node>,
    labels: &mut Vec<String>,
) {
    for (name, module) in tree {
        collect_module_diagram_labels(name, module, nodes, labels);
    }
}

fn collect_module_diagram_labels(
    name: &str,
    module: &Module,
    nodes: &BTreeMap<String, Node>,
    labels: &mut Vec<String>,
) {
    labels.push(name.to_string());
    for component_id in &module.components {
        let Some(node) = nodes.get(component_id) else {
            continue;
        };
        for label in [
            Some(component_id.as_str()),
            Some(node.name.as_str()),
            Some(node.relative_path.as_str()),
            node.display_name.as_deref(),
        ]
        .into_iter()
        .flatten()
        .filter(|label| !label.is_empty())
        {
            labels.push(label.to_string());
        }
    }
    for (child_name, child) in &module.children {
        collect_module_diagram_labels(child_name, child, nodes, labels);
    }
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

#[cfg(test)]
fn markdown_prose_word_count(content: &str) -> usize {
    let counts = markdown_prose_counts(content);
    counts.words + counts.cjk_characters
}

#[derive(Default)]
struct ProseCounts {
    words: usize,
    cjk_characters: usize,
}

fn markdown_prose_counts(content: &str) -> ProseCounts {
    let mut in_fence = false;
    let mut counts = ProseCounts::default();
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
        let line_counts = prose_counts_in_line(trimmed);
        counts.words += line_counts.words;
        counts.cjk_characters += line_counts.cjk_characters;
    }
    counts
}

fn top_level_markdown_pages(output: &Path) -> Result<Vec<String>> {
    let mut pages = Vec::new();
    if !output.is_dir() {
        return Ok(pages);
    }
    for entry in fs::read_dir(output)? {
        let entry = entry?;
        if !entry.file_type()?.is_file()
            || entry.path().extension().and_then(|value| value.to_str()) != Some("md")
        {
            continue;
        }
        if let Some(name) = entry.file_name().to_str() {
            pages.push(name.to_string());
        }
    }
    pages.sort();
    Ok(pages)
}

/// Return local Markdown page destinations while ignoring fenced code blocks,
/// external URLs, fragments, query strings, and link titles.  The returned
/// paths are normalized to the flat output directory used by the wiki.
pub fn markdown_link_targets(content: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut in_fence = false;
    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let mut cursor = 0usize;
        while let Some(relative_start) = line[cursor..].find("](") {
            let start = cursor + relative_start;
            if line[..start].chars().filter(|ch| *ch == '`').count() % 2 == 1 {
                cursor = start + 2;
                continue;
            }
            let target_start = start + 2;
            let Some(relative_end) = line[target_start..].find(')') else {
                break;
            };
            let raw = &line[target_start..target_start + relative_end];
            if let Some(target) = normalize_markdown_target(raw) {
                result.push(target);
            }
            cursor = target_start + relative_end + 1;
        }
    }
    result.sort();
    result.dedup();
    result
}

fn normalize_markdown_target(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let token = if let Some(value) = raw.strip_prefix('<') {
        value.split_once('>')?.0
    } else {
        raw.split_whitespace().next()?
    };
    if token.is_empty()
        || token.contains("://")
        || token.starts_with('/')
        || token.starts_with('#')
        || token.starts_with("../")
    {
        return None;
    }
    let path = token.split(['#', '?']).next().unwrap_or(token);
    let path = path.strip_prefix("./").unwrap_or(path);
    if !path.ends_with(".md") {
        return None;
    }
    Some(percent_decode(path))
}

fn percent_decode(value: &str) -> String {
    let mut bytes = Vec::with_capacity(value.len());
    let mut chars = value.as_bytes().iter().copied();
    while let Some(byte) = chars.next() {
        if byte == b'%' {
            let Some(high) = chars.next() else {
                bytes.push(byte);
                break;
            };
            let Some(low) = chars.next() else {
                bytes.extend([byte, high]);
                break;
            };
            let hex = [high, low];
            if let Ok(text) = std::str::from_utf8(&hex) {
                if let Ok(decoded) = u8::from_str_radix(text, 16) {
                    bytes.push(decoded);
                    continue;
                }
            }
            bytes.extend([byte, high, low]);
        } else {
            bytes.push(byte);
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn prose_counts_in_line(line: &str) -> ProseCounts {
    let mut cleaned = String::new();
    let mut chars = line.chars().peekable();
    let mut in_inline_code = false;
    while let Some(ch) = chars.next() {
        if ch == '`' {
            in_inline_code = !in_inline_code;
            continue;
        }
        if in_inline_code {
            continue;
        }
        if ch == ']' && chars.peek() == Some(&'(') {
            cleaned.push(ch);
            cleaned.push(chars.next().expect("peeked opening parenthesis"));
            for target in chars.by_ref() {
                if target == ')' {
                    break;
                }
            }
            continue;
        }
        cleaned.push(ch);
    }

    let mut counts = ProseCounts::default();
    let mut in_word = false;
    for ch in cleaned.chars() {
        if is_cjk_character(ch) {
            if in_word {
                counts.words += 1;
                in_word = false;
            }
            counts.cjk_characters += 1;
        } else if ch.is_alphanumeric() || ch == '_' {
            in_word = true;
        } else if in_word {
            counts.words += 1;
            in_word = false;
        }
    }
    if in_word {
        counts.words += 1;
    }
    counts
}

fn is_cjk_character(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3040..=0x30ff
            | 0x3400..=0x4dbf
            | 0x4e00..=0x9fff
            | 0xac00..=0xd7af
            | 0xf900..=0xfaff
            | 0x20000..=0x2ffff
    )
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
) -> Result<()> {
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
        )?;
    }
    assigned.extend(module.components.iter().cloned());
    let is_leaf = module.children.is_empty();
    if is_leaf {
        *leaf_count += 1;
    }
    *module_count += 1;
    let doc_path = module_page_filename(name)?;
    order.push(ProcessingItem {
        module: name.to_string(),
        doc_path,
        path: current_path,
        is_leaf,
        children: child_names,
        components: module.components.clone(),
    });
    Ok(())
}

/// Assess the final tree rather than merely checking that its IDs are known.
///
/// A flat tree can cover every selected component and still be unusable as a
/// wiki.  The architecture workflow treats a module as a leaf only when its
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

    // The final tree is an architecture selection, not an exhaustive leaf
    // partition.  `candidate_ids` therefore contains only selected anchors;
    // unselected analyzer candidates are intentionally not errors.
    let orphaned = Vec::<String>::new();
    let mut quality_errors = Vec::new();
    if !metrics.oversized_leaf_modules.is_empty() {
        quality_errors.push(format!(
            "{} leaf module(s) exceed recursive clustering limits",
            metrics.oversized_leaf_modules.len()
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
    _parent_candidate_ids: Option<&BTreeSet<String>>,
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

    let mut descendant_candidate_ids = own_candidate_ids.clone();
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
) -> Result<()> {
    for (name, module) in tree {
        *count += 1;
        *max_depth = (*max_depth).max(depth);
        if module.children.is_empty() {
            *leaf_count += 1;
        }
        let file = module_page_filename(name)?;
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
        )?;
    }
    Ok(())
}

pub fn collect_expected_pages(tree: &ModuleTree, expected: &mut BTreeSet<String>) -> Result<()> {
    for (name, module) in tree {
        expected.insert(module_page_filename(name)?);
        collect_expected_pages(&module.children, expected)?;
    }
    Ok(())
}

fn document_path(state: &SessionState, requested: &str) -> Result<PathBuf> {
    let path = Path::new(requested);
    if requested.is_empty()
        || requested.contains('/')
        || requested.contains('\\')
        || path.is_absolute()
        || path.components().count() != 1
        || path.extension().and_then(|value| value.to_str()) != Some("md")
    {
        return Err(anyhow!(
            "document path must be a flat Markdown filename: {requested}"
        ));
    }
    let output = session::output_dir(state);
    let candidate = output.join(path);
    let canonical_parent = output.canonicalize()?;
    let parent = candidate.parent().unwrap_or(&candidate);
    if parent.exists() {
        let canonical = parent.canonicalize()?;
        if !canonical.starts_with(&canonical_parent) {
            return Err(anyhow!("document path escapes output directory"));
        }
    }
    Ok(candidate)
}

fn canonical_module_stem(name: &str) -> Result<String> {
    if name.is_empty()
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return Err(anyhow!(
            "invalid module name '{name}': use a non-empty ASCII name containing only letters, digits, '_' or '-'"
        ));
    }
    if RESERVED_STEMS.contains(&name) {
        Ok(format!("{name}_module"))
    } else {
        Ok(name.to_string())
    }
}

/// Return the canonical Markdown filename used for a module everywhere in the
/// generation and update workflows.
pub fn module_page_filename(name: &str) -> Result<String> {
    Ok(format!("{}.md", canonical_module_stem(name)?))
}

/// Validate the module-to-page mapping before any page or tree artifacts are
/// written.  The mapping is flat, so names must be unique across the entire
/// tree, not merely among siblings.
pub fn validate_module_page_paths(tree: &ModuleTree) -> Result<()> {
    fn visit(
        modules: &ModuleTree,
        parent_path: &[String],
        seen: &mut BTreeMap<String, String>,
    ) -> Result<()> {
        for (name, module) in modules {
            let page = module_page_filename(name)?;
            let mut path = parent_path.to_vec();
            path.push(name.clone());
            let logical_path = path.join("/");
            if let Some(previous) = seen.insert(page.clone(), logical_path.clone()) {
                return Err(anyhow!(
                    "module page filename collision for '{page}': '{previous}' and '{logical_path}'"
                ));
            }
            visit(&module.children, &path, seen)?;
        }
        Ok(())
    }

    visit(tree, &[], &mut BTreeMap::new())
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
    fn architecture_overview_accepts_grounded_end_to_end_flow() {
        let content = r#"```mermaid
graph TD
    Client[SQL Client] --> Parsers
    Parsers --> AST[Abstract Syntax Tree]
    AST --> Analyzer
    Analyzer --> Planning[Query Planning]
    Planning --> Pipeline[Query Pipeline]
    Pipeline --> Storage[Storage Engine]
    Storage --> Disk[Local or Remote Storage]
```"#;
        let grounded = vec![
            "Parsers".to_string(),
            "Query Planning".to_string(),
            "Query Pipeline".to_string(),
            "Storage Engine".to_string(),
            "Local or Remote Storage".to_string(),
        ];
        let report = validate_mermaid_with_context(content, &grounded, true, true);
        assert_eq!(report.architecture_quality, "pass");
        assert!(report.node_count >= 7);
        assert!(report.edge_count >= 6);
        assert!(report.has_primary_path);
        assert!(report.grounded_node_count >= 4);
    }

    #[test]
    fn architecture_overview_rejects_generic_runtime_plus_build_graph() {
        let content = r#"```mermaid
flowchart LR
    Client[CLI, TUI, app-server, SDK clients] --> Entry[Distribution CLI and Rust entry points]
    Entry --> Protocol[App Server and Public Protocol]
    Protocol --> Core[Agent Core and Context]
    Core --> Exec[Execution and Sandboxing]
    Core --> State[State, History and Files]
    Exec --> Verify[Build and Test Infrastructure]
    State --> Result[Streamed or persisted result]
    Verify --> Release[Release and Smoke Tooling]
```"#;
        let grounded = vec![
            "Distribution CLI".to_string(),
            "App Server and Public Protocol".to_string(),
            "Agent Core and Context".to_string(),
            "Execution and Sandboxing".to_string(),
            "State, History and Files".to_string(),
        ];
        let report = validate_mermaid_with_context(content, &grounded, true, true);
        assert_eq!(report.architecture_quality, "fail");
        assert!(report
            .quality_issues
            .iter()
            .any(|issue| issue.contains("build, test, release")));
    }

    #[test]
    fn reserved_module_names_are_safe() {
        assert_eq!(
            canonical_module_stem("overview").unwrap(),
            "overview_module"
        );
        assert!(canonical_module_stem("Core Services").is_err());
        assert_eq!(
            module_page_filename("overview").unwrap(),
            "overview_module.md"
        );
    }

    #[test]
    fn canonical_page_validation_rejects_collisions_and_non_ascii_keys() {
        assert!(module_page_filename("中文模块").is_err());
        let mut tree = ModuleTree::new();
        tree.insert("overview".to_string(), Module::default());
        tree.insert("overview_module".to_string(), Module::default());
        let error = validate_module_page_paths(&tree).expect_err("page collision must fail");
        assert!(error.to_string().contains("collision"));
    }

    #[test]
    fn prose_counter_counts_natural_cjk_and_ignores_code_and_links() {
        let content = "# 标题\n\n本模块负责读取配置并把请求交给运行时执行。\n\n`inline_code` [实现](Runtime.md)\n\n```rust\nlet ignored = true;\n```\n";
        let count = markdown_prose_word_count(content);
        assert!(
            count >= 20,
            "natural CJK prose should count by characters: {count}"
        );
        assert!(
            count < 40,
            "links and code should not inflate prose: {count}"
        );
    }

    #[test]
    fn page_assessment_uses_language_aware_content_floors() {
        let repeated_cjk = "该模块负责说明职责边界、接口关系和请求执行流程。".repeat(45);
        let cjk_content = format!(
            "# 运行时\n\n## 目的与职责\n\n{repeated_cjk}\n\n## 架构流程\n\n{repeated_cjk}\n"
        );
        let cjk = assess_page(
            "Runtime",
            &Module::default(),
            &cjk_content,
            &BTreeMap::new(),
            PageAssessmentContext {
                is_leaf: true,
                is_overview: false,
                required_links: &[],
                grounded_labels: &[],
                diagram_labels: &[],
            },
        );
        assert_eq!(cjk["prose_count_mode"], json!("cjk_characters"));
        assert_eq!(cjk["prose_floor"], json!(500));
        assert_eq!(cjk["valid"], json!(true), "{cjk}");

        let repeated_english =
            "The runtime owns request parsing, dispatch, state transitions, and result delivery. "
                .repeat(24);
        let english_content = format!(
            "# Runtime\n\n## Purpose\n\n{repeated_english}\n\n## Architecture\n\n{repeated_english}\n"
        );
        let english = assess_page(
            "Runtime",
            &Module::default(),
            &english_content,
            &BTreeMap::new(),
            PageAssessmentContext {
                is_leaf: true,
                is_overview: false,
                required_links: &[],
                grounded_labels: &[],
                diagram_labels: &[],
            },
        );
        assert_eq!(english["prose_count_mode"], json!("english_words"));
        assert_eq!(english["prose_floor"], json!(150));
        assert_eq!(english["valid"], json!(true), "{english}");
    }

    #[test]
    fn page_assessment_requires_two_grounded_anchors_when_available() {
        let ids = vec![
            "src/runtime.rs::Runtime".to_string(),
            "src/runtime.rs::dispatch".to_string(),
        ];
        let module = Module {
            components: ids.clone(),
            ..Module::default()
        };
        let nodes = BTreeMap::from([
            (
                ids[0].clone(),
                Node {
                    id: ids[0].clone(),
                    name: "Runtime".to_string(),
                    relative_path: "src/runtime.rs".to_string(),
                    display_name: Some("struct Runtime".to_string()),
                    ..Node::default()
                },
            ),
            (
                ids[1].clone(),
                Node {
                    id: ids[1].clone(),
                    name: "dispatch".to_string(),
                    relative_path: "src/runtime.rs".to_string(),
                    ..Node::default()
                },
            ),
        ]);
        let prose =
            "The runtime owns request parsing, dispatch, state transitions, and result delivery. "
                .repeat(24);
        let content = format!(
            "# Runtime\n\n## Purpose\n\n{prose}\n\n## Architecture\n\n{prose}\n\nRuntime in `src/runtime.rs`.\n"
        );
        let report = assess_page(
            "Runtime",
            &module,
            &content,
            &nodes,
            PageAssessmentContext {
                is_leaf: true,
                is_overview: false,
                required_links: &[],
                grounded_labels: &["Runtime".to_string()],
                diagram_labels: &["Runtime".to_string()],
            },
        );
        assert_eq!(report["grounded_components"], json!(1));
        assert_eq!(report["valid"], json!(false));
        assert!(report["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|error| error
                .as_str()
                .unwrap_or_default()
                .contains("expected at least 2")));
    }
}
