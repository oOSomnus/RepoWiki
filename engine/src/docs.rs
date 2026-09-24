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
mod quality;
pub use quality::{
    markdown_link_targets, validate_documentation, validate_documentation_report, validate_mermaid,
    MermaidReport,
};

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
                    if review.reason.trim().is_empty() && required {
                        errors.push(format!(
                            "module '{path_label}' has an empty decomposition reason"
                        ));
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
    use super::quality::{
        assess_page, markdown_prose_word_count, validate_mermaid_with_context,
        PageAssessmentContext,
    };
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
