use crate::docs;
use crate::model::{ChangeSet, ModuleTree, Node, UpdateOptions, UpdateRecord};
use crate::session::{self, SessionState};
use anyhow::{anyhow, Result};
use chrono::Utc;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub const VALID_RUNGS: &[&str] = &["0", "1", "2", "3", "3b"];

pub fn validate_options(options: &UpdateOptions) -> Result<()> {
    if !VALID_RUNGS.contains(&options.rung.as_str()) {
        return Err(anyhow!(
            "invalid update rung '{}'; expected one of {}",
            options.rung,
            VALID_RUNGS.join(", ")
        ));
    }

    for (name, value) in [
        ("tau_ren", options.tau_ren),
        ("tau_nb", options.tau_nb),
        ("tau_grow", options.tau_grow),
        ("tau_full", options.tau_full),
        ("tau_tree", options.tau_tree),
    ] {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(anyhow!(
                "{name} must be a finite number between 0 and 1, got {value}"
            ));
        }
    }
    if options.max_diff_tokens == 0 {
        return Err(anyhow!("max_diff_tokens must be greater than zero"));
    }
    Ok(())
}

pub fn plan(state: &SessionState, options: &UpdateOptions) -> Result<Value> {
    validate_options(options)?;
    let started = Instant::now();
    let current_path = current_graph_path(state)?;
    let previous_path = current_path.with_extension("json.prev");
    let current: BTreeMap<String, Node> = session::read_json(&current_path)?;
    let previous: BTreeMap<String, Node> = if previous_path.exists() {
        session::read_json(&previous_path)?
    } else {
        BTreeMap::new()
    };
    let mut diff = graph_diff(&previous, &current, options);
    let active_count = active_ids(&diff).len();
    let total = current.len().max(1);
    let active_leaf_ratio = active_count as f64 / total as f64;
    let structural_count =
        diff.added.len() + diff.deleted.len() + diff.renamed.len() + diff.edge_changes.len();
    let structural_ratio = structural_count as f64 / total as f64;
    diff.active_leaf_ratio = active_leaf_ratio;
    diff.structural_ratio = structural_ratio;
    let fallback = should_fallback(&diff, options).then(|| {
        if structural_ratio >= options.tau_tree {
            "structural-threshold"
        } else {
            "active-leaf-threshold"
        }
        .to_string()
    });
    let growth_ratio = diff.added.len() as f64 / total as f64;
    let growth_reclustered = !diff.added.is_empty() && growth_ratio >= options.tau_grow;
    let reclustered = fallback.is_some() || growth_reclustered;
    let outcome = if previous.is_empty() || fallback.is_some() {
        "full_fallback"
    } else if is_no_change(&diff) {
        "no_change"
    } else {
        "incremental"
    };
    let active = active_ids(&diff);
    let mut write_sets = BTreeMap::new();
    for id in &active {
        let page = module_page_for_node(state, id).unwrap_or_else(|| "overview.md".to_string());
        write_sets.insert(id.clone(), vec![page, "overview.md".to_string()]);
    }
    let record = UpdateRecord {
        started_at: Utc::now().to_rfc3339(),
        finished_at: String::new(),
        outcome: outcome.to_string(),
        options: options.clone(),
        revision: state.analyzed_commit.clone(),
        diff: diff.clone(),
        repair: json!({
            "renamed": diff.renamed.clone(),
            "new_nodes_routed": active,
            "growth_ratio": growth_ratio,
            "growth_threshold": options.tau_grow,
            "reclustered": reclustered,
        }),
        reclustered,
        reports: Vec::new(),
        active: active.clone(),
        write_sets,
        fallback: fallback.clone(),
        verdicts: BTreeMap::new(),
        pages_written: Vec::new(),
        pages_removed: Vec::new(),
        violations: Vec::new(),
        stale_scan: json!({"scanned": false, "stale": []}),
        calls: 0,
        notes: vec!["The host agent owns prose generation and page edits.".to_string()],
        errors: Vec::new(),
        wall_seconds: started.elapsed().as_secs_f64(),
    };
    let root = session::session_root(Path::new(&state.repo_path), &state.session_id);
    session::write_json(&root.join("changes.json"), &diff)?;
    session::write_json(&root.join("update_record_draft.json"), &record)?;
    Ok(json!({
        "session_id": state.session_id,
        "changes_path": root.join("changes.json"),
        "record_path": root.join("update_record_draft.json"),
        "outcome": outcome,
        "active": active,
        "fallback": fallback,
        "diff": diff,
        "update_options": options,
        "growth_ratio": growth_ratio,
        "reclustered": reclustered,
    }))
}

pub fn route(state: &SessionState) -> Result<Value> {
    let root = session::session_root(Path::new(&state.repo_path), &state.session_id);
    let diff: ChangeSet = session::read_json(&root.join("changes.json"))?;
    let nodes: BTreeMap<String, Node> = session::read_json(&root.join("components.json"))?;
    let options = load_update_options(state)?;
    let module_index = module_index(state);
    let mut routes = BTreeMap::new();
    for id in diff
        .added
        .iter()
        .chain(diff.renamed.iter().map(|(_, new)| new))
    {
        let route = nodes
            .get(id)
            .map(|node| route_node(node, &nodes, &module_index, &options))
            .unwrap_or_else(|| "Repository".to_string());
        routes.insert(id.clone(), route);
    }
    let path = root.join("routes.json");
    session::write_json(&path, &routes)?;
    Ok(json!({
        "routes_path": path,
        "routes": routes,
        "neighbor_threshold": options.tau_nb,
    }))
}

pub fn context(state: &SessionState) -> Result<Value> {
    let root = session::session_root(Path::new(&state.repo_path), &state.session_id);
    let diff: ChangeSet = session::read_json(&root.join("changes.json"))?;
    let nodes: BTreeMap<String, Node> = session::read_json(&root.join("components.json"))?;
    let options = load_update_options(state)?;
    let active = active_ids(&diff);
    let reports_dir = root.join("reports");
    fs::create_dir_all(&reports_dir)?;
    let mut reports = Vec::new();
    for id in active {
        if let Some(node) = nodes.get(&id) {
            let up = upstream_ids(&nodes, &id, options.k_hop);
            let referrers = nodes
                .values()
                .filter(|candidate| candidate.depends_on.iter().any(|dep| dep == &id))
                .map(|candidate| candidate.id.clone())
                .collect::<Vec<_>>();
            let own = truncate_text(&node.source_code, options.max_diff_tokens);
            let report = json!({
                "component_id": id,
                "mode": if diff.added.iter().any(|added| added == &id) { "create" } else { "edit" },
                "own": own,
                "up": up,
                "context": {"file": node.relative_path, "language": node.language},
                "referrers": referrers,
                "tree": "See module_tree.json",
                "k_hop": options.k_hop,
                "max_diff_tokens": options.max_diff_tokens,
            });
            let path = reports_dir.join(format!("{}.json", safe_id(&node.id)));
            session::write_json(&path, &report)?;
            reports.push(path.to_string_lossy().into_owned());
        }
    }
    Ok(json!({"reports": reports, "count": reports.len()}))
}

pub fn finalize(state: &SessionState, model: &str, verdicts_path: Option<&Path>) -> Result<Value> {
    let root = session::session_root(Path::new(&state.repo_path), &state.session_id);
    let mut record: UpdateRecord = if root.join("update_record_draft.json").exists() {
        session::read_json(&root.join("update_record_draft.json"))?
    } else {
        UpdateRecord::default()
    };
    if let Some(path) = verdicts_path {
        record.verdicts = read_verdicts(path)?;
    }
    record.reports = list_report_files(&root.join("reports"))?;
    record.finished_at = Utc::now().to_rfc3339();
    record.pages_written = list_markdown_files(&session::output_dir(state))?;
    record.wall_seconds = record
        .started_at
        .parse::<chrono::DateTime<chrono::FixedOffset>>()
        .map(|start| {
            (Utc::now() - start.with_timezone(&Utc))
                .num_milliseconds()
                .max(0) as f64
                / 1000.0
        })
        .unwrap_or(record.wall_seconds);
    let output = session::output_dir(state);
    fs::create_dir_all(&output)?;
    let update_path = output.join("update_record.json");
    session::write_json(&update_path, &record)?;
    let metadata = docs::finalize_metadata(state, model)?;
    Ok(json!({
        "update_record_path": update_path,
        "metadata_path": output.join("metadata.json"),
        "outcome": record.outcome,
        "metadata": metadata,
    }))
}

/// Read the host agent's final page verdicts without making the engine parse
/// natural-language output. Both a direct page map and the reference-style
/// { "verdicts": { ... }, "notes": "..." } envelope are accepted.
fn read_verdicts(path: &Path) -> Result<BTreeMap<String, String>> {
    let value: Value = session::read_json(path)?;
    let raw = value.get("verdicts").unwrap_or(&value);
    let object = raw
        .as_object()
        .ok_or_else(|| anyhow!("verdicts file must contain a JSON object"))?;
    let mut verdicts = BTreeMap::new();
    for (page, value) in object {
        let normalized_page = page.strip_suffix(".md").unwrap_or(page).to_string();
        let verdict = match value {
            Value::String(verdict) => verdict.trim().to_ascii_lowercase(),
            Value::Object(fields) => {
                let name = fields
                    .get("verdict")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow!("verdict for {page} is missing 'verdict'"))?;
                let name = name.trim().to_ascii_lowercase();
                let reason = fields
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if reason.is_empty() {
                    name
                } else {
                    format!("{name}: {reason}")
                }
            }
            _ => return Err(anyhow!("verdict for {page} must be a string or object")),
        };
        let name = verdict.split(':').next().unwrap_or_default().trim();
        if !matches!(name, "no-op" | "patch" | "rewrite" | "create" | "delete") {
            return Err(anyhow!(
                "verdict for {page} must be one of no-op, patch, rewrite, create, delete"
            ));
        }
        verdicts.insert(normalized_page, verdict);
    }
    Ok(verdicts)
}

pub fn current_graph_path(state: &SessionState) -> Result<PathBuf> {
    let directory = session::output_dir(state)
        .join("temp")
        .join("dependency_graphs");
    if !directory.exists() {
        return Err(anyhow!("dependency graph directory is missing"));
    }
    let mut candidates = fs::read_dir(&directory)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().and_then(|ext| ext.to_str()) == Some("json")
                && !path.to_string_lossy().ends_with(".json.prev")
        })
        .collect::<Vec<_>>();
    candidates.sort();
    candidates
        .pop()
        .ok_or_else(|| anyhow!("no dependency graph found"))
}

fn graph_diff(
    previous: &BTreeMap<String, Node>,
    current: &BTreeMap<String, Node>,
    options: &UpdateOptions,
) -> ChangeSet {
    let previous_ids = previous.keys().cloned().collect::<BTreeSet<_>>();
    let current_ids = current.keys().cloned().collect::<BTreeSet<_>>();
    let added = current_ids
        .difference(&previous_ids)
        .cloned()
        .collect::<Vec<_>>();
    let deleted = previous_ids
        .difference(&current_ids)
        .cloned()
        .collect::<Vec<_>>();
    let mut modified_interface = Vec::new();
    let mut modified_body = Vec::new();
    let mut edge_changes = Vec::new();
    for id in previous_ids.intersection(&current_ids) {
        let old = &previous[id];
        let new = &current[id];
        if signature(old) != signature(new) {
            modified_interface.push(id.clone());
        } else if old.source_code != new.source_code {
            modified_body.push(id.clone());
        }
        if old.depends_on != new.depends_on {
            edge_changes.push(id.clone());
        }
    }
    let mut candidates = Vec::new();
    for old_id in &deleted {
        let Some(old) = previous.get(old_id) else {
            continue;
        };
        let old_tokens = source_tokens(&old.source_code);
        if old_tokens.is_empty() {
            continue;
        }
        for new_id in &added {
            let Some(new) = current.get(new_id) else {
                continue;
            };
            if old.component_type != new.component_type || old.language != new.language {
                continue;
            }
            let new_tokens = source_tokens(&new.source_code);
            if new_tokens.is_empty() {
                continue;
            }
            let shorter = old_tokens.len().min(new_tokens.len()) as f64;
            let longer = old_tokens.len().max(new_tokens.len()) as f64;
            if shorter / longer < options.tau_ren * 0.9 {
                continue;
            }
            let similarity = body_similarity(&old_tokens, &new_tokens);
            if similarity >= options.tau_ren {
                candidates.push((similarity, old_id.clone(), new_id.clone()));
            }
        }
    }
    candidates.sort_by(|left, right| {
        right
            .0
            .partial_cmp(&left.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.1.cmp(&right.1))
            .then_with(|| left.2.cmp(&right.2))
    });
    let mut used_old = BTreeSet::new();
    let mut used_new = BTreeSet::new();
    let mut renamed = Vec::new();
    for (_, old_id, new_id) in candidates {
        if used_old.insert(old_id.clone()) && used_new.insert(new_id.clone()) {
            renamed.push((old_id, new_id));
        }
    }
    let renamed_old = renamed
        .iter()
        .map(|(old_id, _)| old_id)
        .collect::<BTreeSet<_>>();
    let renamed_new = renamed
        .iter()
        .map(|(_, new_id)| new_id)
        .collect::<BTreeSet<_>>();
    let added = added
        .into_iter()
        .filter(|id| !renamed_new.contains(id))
        .collect();
    let deleted = deleted
        .into_iter()
        .filter(|id| !renamed_old.contains(id))
        .collect();
    ChangeSet {
        added,
        deleted,
        modified_interface,
        modified_body,
        edge_changes,
        renamed,
        ..Default::default()
    }
}

fn signature(node: &Node) -> String {
    format!(
        "{}|{}|{}",
        node.name,
        node.parameters.join(","),
        node.base_classes.join(",")
    )
}

fn active_ids(diff: &ChangeSet) -> Vec<String> {
    diff.added
        .iter()
        .chain(diff.modified_interface.iter())
        .chain(diff.modified_body.iter())
        .chain(diff.edge_changes.iter())
        .chain(diff.renamed.iter().map(|(_, new_id)| new_id))
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn should_fallback(diff: &ChangeSet, options: &UpdateOptions) -> bool {
    diff.active_leaf_ratio >= options.tau_full || diff.structural_ratio >= options.tau_tree
}

fn is_no_change(diff: &ChangeSet) -> bool {
    diff.added.is_empty()
        && diff.deleted.is_empty()
        && diff.renamed.is_empty()
        && diff.modified_interface.is_empty()
        && diff.modified_body.is_empty()
        && diff.edge_changes.is_empty()
}

fn module_page_for_node(state: &SessionState, id: &str) -> Option<String> {
    let tree = docs::read_tree_file(&session::module_tree_path(state)).ok()?;
    fn visit(name: &str, module: &crate::model::Module, id: &str) -> Option<String> {
        if module.components.iter().any(|component| component == id) {
            return Some(docs::module_page_filename(name));
        }
        for (child_name, child) in &module.children {
            if let Some(value) = visit(child_name, child, id) {
                return Some(value);
            }
        }
        None
    }
    tree.iter()
        .find_map(|(name, module)| visit(name, module, id))
}

fn load_update_options(state: &SessionState) -> Result<UpdateOptions> {
    let root = session::session_root(Path::new(&state.repo_path), &state.session_id);
    let path = root.join("update_record_draft.json");
    let options = if path.exists() {
        let record: UpdateRecord = session::read_json(&path)?;
        record.options
    } else {
        UpdateOptions::default()
    };
    validate_options(&options)?;
    Ok(options)
}

fn module_index(state: &SessionState) -> BTreeMap<String, String> {
    let path = session::module_tree_path(state);
    let Ok(tree) = docs::read_tree_file(&path) else {
        return BTreeMap::new();
    };
    let mut index = BTreeMap::new();
    index_modules(&tree, &mut index);
    index
}

fn index_modules(tree: &ModuleTree, index: &mut BTreeMap<String, String>) {
    for (name, module) in tree {
        for component in &module.components {
            index.insert(component.clone(), name.clone());
        }
        index_modules(&module.children, index);
    }
}

fn route_node(
    node: &Node,
    nodes: &BTreeMap<String, Node>,
    module_index: &BTreeMap<String, String>,
    options: &UpdateOptions,
) -> String {
    let mut counts = BTreeMap::<String, usize>::new();
    for dependency in &node.depends_on {
        if let Some(module) = module_index.get(dependency) {
            *counts.entry(module.clone()).or_default() += 1;
        }
    }
    if let Some((module, count)) = counts
        .into_iter()
        .max_by(|left, right| left.1.cmp(&right.1).then_with(|| right.0.cmp(&left.0)))
    {
        let total = node
            .depends_on
            .iter()
            .filter(|dependency| nodes.contains_key(*dependency))
            .count();
        if total > 0 && count as f64 / total as f64 >= options.tau_nb {
            return module;
        }
    }
    node.relative_path
        .split('/')
        .next()
        .unwrap_or("Repository")
        .to_string()
}

fn upstream_ids(nodes: &BTreeMap<String, Node>, start: &str, max_hops: usize) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut frontier = nodes
        .get(start)
        .map(|node| node.depends_on.iter().cloned().collect::<BTreeSet<_>>())
        .unwrap_or_default();
    for _ in 0..max_hops {
        if frontier.is_empty() {
            break;
        }
        let mut next = BTreeSet::new();
        for dependency in frontier {
            if seen.insert(dependency.clone()) {
                if let Some(node) = nodes.get(&dependency) {
                    next.extend(node.depends_on.iter().cloned());
                }
            }
        }
        frontier = next;
    }
    seen.into_iter().collect()
}

fn source_tokens(source: &str) -> Vec<&str> {
    source
        .split(|character: char| !character.is_alphanumeric() && character != '_')
        .filter(|token| !token.is_empty())
        .collect()
}

fn body_similarity(left: &[&str], right: &[&str]) -> f64 {
    if left.is_empty() && right.is_empty() {
        return 1.0;
    }
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let mut previous = vec![0usize; right.len() + 1];
    for left_token in left {
        let mut current = vec![0usize; right.len() + 1];
        for (index, right_token) in right.iter().enumerate() {
            current[index + 1] = if left_token == right_token {
                previous[index] + 1
            } else {
                previous[index + 1].max(current[index])
            };
        }
        previous = current;
    }
    (2 * previous[right.len()]) as f64 / (left.len() + right.len()) as f64
}

fn truncate_text(text: &str, max_tokens: usize) -> String {
    let max_chars = max_tokens.saturating_mul(4);
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let marker = "\n... [diff truncated] ...\n";
    if max_chars <= marker.chars().count() {
        return text.chars().take(max_chars).collect();
    }
    let available = max_chars - marker.chars().count();
    let head_len = available / 2;
    let tail_len = available - head_len;
    let head = text.chars().take(head_len).collect::<String>();
    let tail = text
        .chars()
        .rev()
        .take(tail_len)
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    format!("{head}{marker}{tail}")
}

fn list_markdown_files(output: &Path) -> Result<Vec<String>> {
    let mut files = Vec::new();
    if !output.exists() {
        return Ok(files);
    }
    for entry in fs::read_dir(output)? {
        let entry = entry?;
        if entry.path().extension().and_then(|ext| ext.to_str()) == Some("md") {
            files.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    files.sort();
    Ok(files)
}

fn list_report_files(reports: &Path) -> Result<Vec<String>> {
    let mut files = Vec::new();
    if !reports.exists() {
        return Ok(files);
    }
    for entry in fs::read_dir(reports)? {
        let entry = entry?;
        if entry.path().extension().and_then(|ext| ext.to_str()) == Some("json") {
            files.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    files.sort();
    Ok(files)
}

fn safe_id(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_body_and_interface_changes() {
        let old = Node {
            id: "a.py::run".to_string(),
            source_code: "def run(x):\n return 1".to_string(),
            ..Default::default()
        };
        let new = Node {
            id: "a.py::run".to_string(),
            source_code: "def run(x):\n return 2".to_string(),
            ..Default::default()
        };
        let diff = graph_diff(
            &BTreeMap::from([(old.id.clone(), old)]),
            &BTreeMap::from([(new.id.clone(), new)]),
            &UpdateOptions::default(),
        );
        assert_eq!(diff.modified_body, vec!["a.py::run"]);
    }

    #[test]
    fn rejects_invalid_rung_and_thresholds() {
        let invalid_rung = UpdateOptions {
            rung: "4".to_string(),
            ..Default::default()
        };
        assert!(validate_options(&invalid_rung).is_err());

        let invalid_threshold = UpdateOptions {
            rung: "3".to_string(),
            tau_ren: f64::NAN,
            ..Default::default()
        };
        assert!(validate_options(&invalid_threshold).is_err());
    }

    #[test]
    fn rename_similarity_uses_tau_ren() {
        let old = Node {
            id: "a.py::old".to_string(),
            name: "old".to_string(),
            component_type: "function".to_string(),
            language: "Python".to_string(),
            source_code: "def old(): return alpha + beta + gamma".to_string(),
            ..Default::default()
        };
        let new = Node {
            id: "a.py::new".to_string(),
            name: "new".to_string(),
            component_type: "function".to_string(),
            language: "Python".to_string(),
            source_code: "def new(): return alpha + beta + delta".to_string(),
            ..Default::default()
        };
        let previous = BTreeMap::from([(old.id.clone(), old)]);
        let current = BTreeMap::from([(new.id.clone(), new)]);

        let strict = UpdateOptions {
            tau_ren: 0.95,
            ..Default::default()
        };
        assert!(graph_diff(&previous, &current, &strict).renamed.is_empty());

        let mut relaxed = strict.clone();
        relaxed.tau_ren = 0.5;
        assert_eq!(
            graph_diff(&previous, &current, &relaxed).renamed,
            vec![("a.py::old".to_string(), "a.py::new".to_string())]
        );
    }

    #[test]
    fn fallback_thresholds_are_applied_for_rung_three() {
        let diff = ChangeSet {
            active_leaf_ratio: 0.4,
            structural_ratio: 0.1,
            ..Default::default()
        };
        let options = UpdateOptions {
            rung: "3".to_string(),
            tau_full: 0.5,
            ..Default::default()
        };
        assert!(!should_fallback(&diff, &options));
        let lower_threshold = UpdateOptions {
            tau_full: 0.3,
            ..options
        };
        assert!(should_fallback(&diff, &lower_threshold));
    }

    #[test]
    fn verdict_file_accepts_reference_envelope_and_normalizes_pages() {
        let directory = tempfile::tempdir().expect("temporary verdict directory");
        let path = directory.path().join("verdicts.json");
        session::write_json(
            &path,
            &json!({
                "verdicts": {
                    "Service.md": {"verdict": "patch", "reason": "updated signature"},
                    "overview.md": "no-op"
                },
                "notes": "fixed replay"
            }),
        )
        .expect("write verdicts");

        let verdicts = read_verdicts(&path).expect("parse verdicts");
        assert_eq!(verdicts["Service"], "patch: updated signature");
        assert_eq!(verdicts["overview"], "no-op");
    }
}
