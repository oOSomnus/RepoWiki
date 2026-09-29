use super::{
    collect_expected_pages, module_page_id, overview_page_id, page_file_path,
    validate_module_page_paths,
};
use crate::dokuwiki;
use crate::model::{Module, ModuleTree, Node};
use crate::session::{self, files, SessionState};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
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

/// Verify the file-side completion contract before a session is removed.
///
/// The DokuWiki parser is the source of truth for internal link targets. This
/// gate also checks that the canonical page IDs derived from the saved tree
/// match the pages present in the edition's DokuWiki store.
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
/// the session. This leaves actionable diagnostics available for page repair.
pub fn validate_documentation_report(state: &SessionState) -> Result<Value> {
    let output = session::output_dir(state);
    for required in ["module_tree.json", "first_module_tree.json"] {
        if !output.join(required).is_file() {
            return Err(anyhow!("incomplete documentation: missing {required}"));
        }
    }

    let validation_path = session::session_file(state, files::MODULE_TREE_VALIDATION)?;
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
    let mut errors = Vec::new();
    let canonical_tree_valid = match validate_module_page_paths(&state.wiki_id, &tree) {
        Ok(()) => true,
        Err(error) => {
            errors.push(format!("invalid canonical module page IDs: {error}"));
            false
        }
    };
    let mut expected = BTreeSet::new();
    let expected_ids_valid = match collect_expected_pages(&state.wiki_id, &tree, &mut expected) {
        Ok(()) => true,
        Err(error) => {
            errors.push(format!("cannot derive canonical module page IDs: {error}"));
            false
        }
    };
    let overview_id = overview_page_id(&state.wiki_id)?;
    expected.insert(overview_id);

    let actual_files = collect_dokuwiki_pages(&output, &state.wiki_id)?;
    let mut actual_page_ids = BTreeSet::new();
    let mut actual_paths_by_id = BTreeMap::<String, Vec<PathBuf>>::new();
    let mut actual_ids_canonical = true;
    for file in &actual_files {
        actual_page_ids.insert(file.page_id.clone());
        actual_paths_by_id
            .entry(file.page_id.clone())
            .or_default()
            .push(file.path.clone());
        if !file.canonical {
            actual_ids_canonical = false;
            errors.push(format!(
                "non-canonical DokuWiki page file ID: {}",
                file.page_id
            ));
        }
    }
    let duplicate_page_ids = actual_paths_by_id
        .iter()
        .filter(|(_, paths)| paths.len() > 1)
        .map(|(page_id, _)| page_id.clone())
        .collect::<Vec<_>>();
    for page_id in &duplicate_page_ids {
        errors.push(format!("duplicate DokuWiki page ID: {page_id}"));
    }

    let missing_page_ids = expected
        .iter()
        .filter(|page_id| {
            page_file_path(&output, &state.wiki_id, page_id)
                .ok()
                .is_none_or(|expected_path| {
                    !actual_files
                        .iter()
                        .any(|file| file.canonical && file.path == expected_path)
                })
        })
        .cloned()
        .collect::<Vec<_>>();
    for page_id in &missing_page_ids {
        errors.push(format!("missing DokuWiki page ID: {page_id}"));
    }

    let extra_dokuwiki_pages = actual_page_ids
        .difference(&expected)
        .cloned()
        .collect::<Vec<_>>();
    for page_id in &extra_dokuwiki_pages {
        errors.push(format!("unexpected DokuWiki page ID: {page_id}"));
    }

    let nodes: BTreeMap<String, Node> =
        session::read_json(&session::session_file(state, files::COMPONENTS)?)?;
    let mut page_sources = BTreeMap::new();
    let mut parsed_page_ids = BTreeSet::new();
    let mut dokuwiki_links = Vec::new();
    let mut broken_dokuwiki_links = Vec::new();
    let context = dokuwiki::session_context(state)?;
    for file in &actual_files {
        if !file.safe_to_read {
            let message = format!(
                "refusing to read DokuWiki page ID {} through a symlink",
                file.page_id
            );
            errors.push(message.clone());
            page_sources.insert(
                file.page_id.clone(),
                PageSource {
                    content: String::new(),
                    links: None,
                    parser_error: Some(message),
                },
            );
            continue;
        }
        let content = match fs::read_to_string(&file.path) {
            Ok(content) => content,
            Err(error) => {
                let message = format!("cannot read DokuWiki page ID {}: {error}", file.page_id);
                errors.push(message.clone());
                page_sources.insert(
                    file.page_id.clone(),
                    PageSource {
                        content: String::new(),
                        links: None,
                        parser_error: Some(message),
                    },
                );
                continue;
            }
        };
        match context.parse(&file.page_id, &content) {
            Ok(parsed) => {
                parsed_page_ids.insert(file.page_id.clone());
                for target_page_id in &parsed.links {
                    let link = json!({
                        "page_id": file.page_id,
                        "target_page_id": target_page_id,
                    });
                    dokuwiki_links.push(link.clone());
                    if !expected.contains(target_page_id) {
                        broken_dokuwiki_links.push(link);
                    }
                }
                page_sources.insert(
                    file.page_id.clone(),
                    PageSource {
                        content,
                        links: Some(parsed.links),
                        parser_error: None,
                    },
                );
            }
            Err(error) => {
                let message = format!(
                    "DokuWiki parser failed for page ID {}: {error}",
                    file.page_id
                );
                errors.push(message.clone());
                page_sources.insert(
                    file.page_id.clone(),
                    PageSource {
                        content,
                        links: None,
                        parser_error: Some(message),
                    },
                );
            }
        }
    }
    for link in &broken_dokuwiki_links {
        errors.push(format!(
            "broken DokuWiki link in {}: {}",
            link["page_id"].as_str().unwrap_or_default(),
            link["target_page_id"].as_str().unwrap_or_default()
        ));
    }

    let mut builder = DocumentationReportBuilder::new(&nodes, &page_sources);
    if let Err(error) = builder.visit_modules(&state.wiki_id, &tree, &[]) {
        errors.push(format!("cannot assess module pages: {error}"));
    }
    if let Err(error) = builder.visit_overview(&state.wiki_id, &tree) {
        errors.push(format!("cannot assess overview page: {error}"));
    }
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
    errors.extend(builder_errors);
    errors.sort();
    errors.dedup();

    let decomposition_review_warnings = validation["decomposition_review"]["warnings"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let canonical_page_ids_valid = canonical_tree_valid
        && expected_ids_valid
        && actual_ids_canonical
        && duplicate_page_ids.is_empty();
    let source_grounding_valid = pages.values().all(|page| {
        page["grounded_components"].as_u64().unwrap_or_default()
            >= page["component_count"].as_u64().unwrap_or_default().min(2)
    });
    let semantic_sections_valid = pages
        .values()
        .all(|page| page["semantic_sections"].as_u64().unwrap_or_default() >= 2);
    let parent_child_links_valid = pages
        .values()
        .all(|page| page["missing_links"].as_array().is_some_and(Vec::is_empty));
    let architecture_diagrams_valid = pages
        .values()
        .all(|page| page["architecture_diagram"]["architecture_quality"].as_str() != Some("fail"));
    let template_only_rejected = pages
        .values()
        .all(|page| page["boilerplate_detected"].as_bool() != Some(true));

    let report = json!({
        "valid": errors.is_empty(),
        "wiki_id": state.wiki_id,
        "errors": errors,
        "expected_page_ids": expected,
        "actual_page_ids": actual_page_ids,
        "missing_page_ids": missing_page_ids,
        "extra_dokuwiki_pages": extra_dokuwiki_pages,
        "duplicate_page_ids": duplicate_page_ids,
        "parsed_page_ids": parsed_page_ids,
        "dokuwiki_links": dokuwiki_links,
        "broken_dokuwiki_links": broken_dokuwiki_links,
        "warnings": decomposition_review_warnings,
        "prose_count_mode": "language-aware-v2",
        "page_count": page_count,
        "valid_page_count": valid_page_count,
        "explanatory_page_count": explanatory_page_count,
        "mermaid_page_count": mermaid_page_count,
        "grounded_page_count": grounded_page_count,
        "pages": Value::Object(pages),
        "checks": {
            "source_grounding": source_grounding_valid,
            "semantic_sections": semantic_sections_valid,
            "parent_child_links": parent_child_links_valid,
            "architecture_diagrams": architecture_diagrams_valid,
            "template_only_rejection": template_only_rejected,
            "canonical_page_ids": canonical_page_ids_valid,
            "module_decomposition_review": validation["decomposition_review"]["required"]
                == Value::Bool(true),
            "no_extra_dokuwiki_pages": extra_dokuwiki_pages.is_empty(),
            "no_broken_dokuwiki_links": broken_dokuwiki_links.is_empty(),
        },
    });
    session::write_json(
        &session::session_file(state, files::DOCUMENTATION_VALIDATION)?,
        &report,
    )?;
    Ok(report)
}

fn analyze_mermaid(
    content: &str,
    grounded_labels: &[String],
    strict: bool,
    forbid_support_nodes: bool,
) -> MermaidReport {
    let (blocks, balanced) = extract_mermaid_blocks(content);

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
        quality_issues.push("unbalanced native <mermaid> tags".to_string());
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

fn extract_mermaid_blocks(content: &str) -> (Vec<String>, bool) {
    let ignored_ranges = opaque_dokuwiki_ranges(content);
    let mut blocks = Vec::new();
    let mut current = None::<usize>;
    let mut balanced = true;
    let mut cursor = 0;

    loop {
        let open = next_unignored_markup(content, "<mermaid>", cursor, &ignored_ranges);
        let close = next_unignored_markup(content, "</mermaid>", cursor, &ignored_ranges);
        let next = match (open, close) {
            (None, None) => break,
            (Some(open), None) => (open, true),
            (None, Some(close)) => (close, false),
            (Some(open), Some(close)) if open <= close => (open, true),
            (Some(_), Some(close)) => (close, false),
        };
        let (position, opening) = next;
        if opening {
            if current.is_some() {
                balanced = false;
            } else {
                current = Some(position + "<mermaid>".len());
            }
            cursor = position + "<mermaid>".len();
        } else {
            if let Some(start) = current.take() {
                blocks.push(content[start..position].to_string());
            } else {
                balanced = false;
            }
            cursor = position + "</mermaid>".len();
        }
    }
    if current.is_some() {
        balanced = false;
    }
    (blocks, balanced)
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

struct PageSource {
    content: String,
    links: Option<Vec<String>>,
    parser_error: Option<String>,
}

struct DocumentationReportBuilder<'a> {
    nodes: &'a BTreeMap<String, Node>,
    page_sources: &'a BTreeMap<String, PageSource>,
    pages: serde_json::Map<String, Value>,
    errors: Vec<String>,
    page_count: usize,
    valid_page_count: usize,
    explanatory_page_count: usize,
    mermaid_page_count: usize,
    grounded_page_count: usize,
}

impl<'a> DocumentationReportBuilder<'a> {
    fn new(
        nodes: &'a BTreeMap<String, Node>,
        page_sources: &'a BTreeMap<String, PageSource>,
    ) -> Self {
        Self {
            nodes,
            page_sources,
            pages: serde_json::Map::new(),
            errors: Vec::new(),
            page_count: 0,
            valid_page_count: 0,
            explanatory_page_count: 0,
            mermaid_page_count: 0,
            grounded_page_count: 0,
        }
    }

    fn visit_modules(
        &mut self,
        wiki_id: &str,
        modules: &ModuleTree,
        parent_path: &[String],
    ) -> Result<()> {
        for (name, module) in modules {
            let mut module_path = parent_path.to_vec();
            module_path.push(name.clone());
            let page_id = module_page_id(wiki_id, &module_path)?;
            let source = self.page_sources.get(&page_id);
            let content = source.map_or("", |source| source.content.as_str());
            let actual_links = source
                .and_then(|source| source.links.as_deref())
                .unwrap_or(&[]);
            let required_links = module
                .children
                .keys()
                .map(|child| {
                    let mut child_path = module_path.clone();
                    child_path.push(child.clone());
                    module_page_id(wiki_id, &child_path)
                })
                .collect::<Result<Vec<_>>>()?;
            let labels = module
                .children
                .keys()
                .cloned()
                .chain(std::iter::once(name.clone()))
                .collect::<Vec<_>>();
            let mut diagram_labels = labels.clone();
            collect_module_diagram_labels(name, module, self.nodes, &mut diagram_labels);
            let mut result = assess_page(
                name,
                module,
                content,
                self.nodes,
                PageAssessmentContext {
                    is_leaf: module.children.is_empty(),
                    is_overview: false,
                    required_links: &required_links,
                    actual_links,
                    grounded_labels: &labels,
                    diagram_labels: &diagram_labels,
                },
            );
            if let Some(error) = Self::attach_parser_result(&page_id, source, &mut result) {
                self.errors.push(error);
            }
            self.record_page(&page_id, result);
            self.visit_modules(wiki_id, &module.children, &module_path)?;
        }
        Ok(())
    }

    fn visit_overview(&mut self, wiki_id: &str, tree: &ModuleTree) -> Result<()> {
        let page_id = overview_page_id(wiki_id)?;
        let source = self.page_sources.get(&page_id);
        let content = source.map_or("", |source| source.content.as_str());
        let actual_links = source
            .and_then(|source| source.links.as_deref())
            .unwrap_or(&[]);
        let overview_links = tree
            .keys()
            .map(|name| module_page_id(wiki_id, std::slice::from_ref(name)))
            .collect::<Result<Vec<_>>>()?;
        let mut labels = Vec::new();
        collect_module_labels(tree, &mut labels);
        let mut diagram_labels = labels.clone();
        collect_tree_diagram_labels(tree, self.nodes, &mut diagram_labels);
        let mut result = assess_page(
            "Repository overview",
            &Module::default(),
            content,
            self.nodes,
            PageAssessmentContext {
                is_leaf: false,
                is_overview: true,
                required_links: &overview_links,
                actual_links,
                grounded_labels: &labels,
                diagram_labels: &diagram_labels,
            },
        );
        if let Some(error) = Self::attach_parser_result(&page_id, source, &mut result) {
            self.errors.push(error);
        }
        self.record_page(&page_id, result);
        Ok(())
    }

    fn attach_parser_result(
        page_id: &str,
        source: Option<&PageSource>,
        result: &mut Value,
    ) -> Option<String> {
        match source {
            Some(PageSource { links: Some(_), .. }) => {
                result["parser_succeeded"] = Value::Bool(true);
                None
            }
            Some(source) => {
                result["parser_succeeded"] = Value::Bool(false);
                let error = source
                    .parser_error
                    .as_deref()
                    .unwrap_or("DokuWiki parser did not return a parsed page");
                result["parser_error"] = json!(error);
                if let Some(errors) = result["errors"].as_array_mut() {
                    errors.push(json!(error));
                }
                result["valid"] = Value::Bool(false);
                Some(format!("{page_id}: {error}"))
            }
            None => {
                result["parser_succeeded"] = Value::Null;
                if let Some(errors) = result["errors"].as_array_mut() {
                    errors.push(json!("page is missing from the DokuWiki store"));
                }
                result["valid"] = Value::Bool(false);
                Some(format!(
                    "{page_id}: page is missing from the DokuWiki store"
                ))
            }
        }
    }

    fn record_page(&mut self, page_id: &str, mut result: Value) {
        result["page_id"] = json!(page_id);
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
                self.errors.push(format!("{page_id}: {error}"));
            }
        }
        self.page_count += 1;
        self.pages.insert(page_id.to_string(), result);
    }
}

/// Check whether a generated page contains an explanation grounded in the
/// analyzed repository. This is deliberately a small structural heuristic,
/// not an attempt to judge prose with another model. It catches the failure
/// mode where a host calls prompt get but then writes a fixed component list
/// or a one-line overview instead of using the model response.
pub(super) struct PageAssessmentContext<'a> {
    pub(super) is_leaf: bool,
    pub(super) is_overview: bool,
    pub(super) required_links: &'a [String],
    pub(super) actual_links: &'a [String],
    pub(super) grounded_labels: &'a [String],
    pub(super) diagram_labels: &'a [String],
}

pub(super) fn assess_page(
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
        actual_links,
        grounded_labels,
        diagram_labels,
    } = context;
    let headings = dokuwiki_headings(content);
    let lower = content.to_ascii_lowercase();
    let mermaid = analyze_mermaid(content, diagram_labels, !is_leaf, is_overview);
    let prose = prose_counts(content);
    let cjk_mode = prose.cjk_characters > prose.words;
    let prose_words = if cjk_mode {
        prose.cjk_characters
    } else {
        prose.words
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
    if mermaid.architecture_quality == "fail" {
        page_errors.extend(mermaid.quality_issues.iter().cloned());
    }
    if template_only {
        page_errors.push(
            "looks like a generated component-list template rather than an explanatory page"
                .to_string(),
        );
    }
    let missing_links = required_links
        .iter()
        .filter(|link| !actual_links.contains(link))
        .cloned()
        .collect::<Vec<_>>();
    if !missing_links.is_empty() {
        page_errors.push(format!(
            "DokuWiki parser found no links to required child/module page IDs: {}",
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
        "architecture_diagram": mermaid,
        "links": actual_links,
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

fn dokuwiki_headings(content: &str) -> Vec<String> {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            let leading = trimmed.bytes().take_while(|byte| *byte == b'=').count();
            let trailing = trimmed
                .bytes()
                .rev()
                .take_while(|byte| *byte == b'=')
                .count();
            if leading < 2 || leading != trailing || leading * 2 >= trimmed.len() {
                return None;
            }
            let heading = trimmed[leading..trimmed.len() - trailing].trim();
            (!heading.is_empty()).then(|| dokuwiki_visible_text(heading).to_ascii_lowercase())
        })
        .collect()
}

fn has_heading_term(headings: &[String], terms: &[&str]) -> bool {
    headings
        .iter()
        .any(|heading| terms.iter().any(|term| heading.contains(term)))
}

#[derive(Default)]
struct ProseCounts {
    words: usize,
    cjk_characters: usize,
}

fn prose_counts(content: &str) -> ProseCounts {
    let mut opaque_ranges = opaque_dokuwiki_ranges(content);
    let mermaid_ranges = mermaid_source_ranges(content, &opaque_ranges);
    opaque_ranges.extend(mermaid_ranges);
    opaque_ranges.sort_unstable();
    let mut counts = ProseCounts::default();
    let mut line_start = 0;
    for raw_line in content.split_inclusive('\n') {
        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        let line = line.strip_suffix('\r').unwrap_or(line);
        let trimmed = line.trim();
        let indentation = line.len() - line.trim_start().len();
        if trimmed.is_empty()
            || is_dokuwiki_heading(trimmed)
            || trimmed.starts_with('*')
            || trimmed.starts_with('-')
            || trimmed.starts_with('>')
            || trimmed.starts_with('|')
            || trimmed.starts_with('^')
            || trimmed.starts_with("----")
            || (indentation >= 2 && !trimmed.starts_with('*') && !trimmed.starts_with('-'))
        {
            line_start += raw_line.len();
            continue;
        }
        let line_end = line_start + line.len();
        let mut visible = String::new();
        let mut cursor = line_start;
        for (start, end) in opaque_ranges
            .iter()
            .filter(|(start, end)| *start < line_end && *end > line_start)
        {
            let visible_end = (*start).min(line_end);
            if cursor < visible_end {
                visible.push_str(&content[cursor..visible_end]);
            }
            cursor = cursor.max((*end).min(line_end));
        }
        if cursor < line_end {
            visible.push_str(&content[cursor..line_end]);
        }
        let line_counts = prose_counts_in_line(&visible);
        counts.words += line_counts.words;
        counts.cjk_characters += line_counts.cjk_characters;
        line_start += raw_line.len();
    }
    counts
}

fn is_dokuwiki_heading(line: &str) -> bool {
    let leading = line.bytes().take_while(|byte| *byte == b'=').count();
    let trailing = line.bytes().rev().take_while(|byte| *byte == b'=').count();
    leading >= 2 && leading == trailing && leading * 2 < line.len()
}

struct DokuwikiPageFile {
    page_id: String,
    path: PathBuf,
    canonical: bool,
    safe_to_read: bool,
}

fn collect_dokuwiki_pages(output: &Path, wiki_id: &str) -> Result<Vec<DokuwikiPageFile>> {
    fn visit(directory: &Path, files: &mut Vec<(PathBuf, bool)>) -> Result<()> {
        if !directory.exists() {
            return Ok(());
        }
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                visit(&path, files)?;
            } else if path.extension().and_then(|value| value.to_str()) == Some("txt")
                && (file_type.is_file() || file_type.is_symlink())
            {
                files.push((path, file_type.is_file()));
            }
        }
        Ok(())
    }

    let page_root = output.join("dokuwiki/data/pages");
    if page_root.exists() {
        let canonical_output = output.canonicalize()?;
        if !page_root.canonicalize()?.starts_with(&canonical_output) {
            return Err(anyhow!(
                "DokuWiki pages directory escapes edition output: {}",
                page_root.display()
            ));
        }
    }
    let mut paths = Vec::new();
    visit(&page_root, &mut paths)?;
    paths.sort_by(|left, right| left.0.cmp(&right.0));
    paths
        .into_iter()
        .map(|(path, safe_to_read)| {
            let page_id = page_id_from_file(&page_root, &path)?;
            let canonical = safe_to_read
                && page_file_path(output, wiki_id, &page_id)
                    .is_ok_and(|expected_path| expected_path == path);
            Ok(DokuwikiPageFile {
                page_id,
                path,
                canonical,
                safe_to_read,
            })
        })
        .collect()
}

fn page_id_from_file(page_root: &Path, path: &Path) -> Result<String> {
    let relative = path.strip_prefix(page_root)?;
    let mut segments = relative
        .parent()
        .into_iter()
        .flat_map(|parent| parent.components())
        .filter_map(|component| match component {
            std::path::Component::Normal(segment) => Some(segment.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let page_name = path
        .file_stem()
        .ok_or_else(|| anyhow!("DokuWiki page file has no name: {}", path.display()))?
        .to_string_lossy()
        .into_owned();
    segments.push(page_name);
    Ok(segments.join(":"))
}

fn opaque_dokuwiki_ranges(content: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    for tag in ["code", "file", "nowiki"] {
        let opening = format!("<{tag}");
        let closing = format!("</{tag}>");
        let mut cursor = 0;
        while let Some(relative_start) = content[cursor..].find(&opening) {
            let start = cursor + relative_start;
            let tag_end = start + opening.len();
            if content
                .as_bytes()
                .get(tag_end)
                .is_some_and(|byte| !byte.is_ascii_whitespace() && *byte != b'>')
            {
                cursor = tag_end;
                continue;
            }
            let Some(relative_end) = content[tag_end..].find('>') else {
                ranges.push((start, content.len()));
                break;
            };
            let body_start = tag_end + relative_end + 1;
            if let Some(relative_close) = content[body_start..].find(&closing) {
                let end = body_start + relative_close + closing.len();
                ranges.push((start, end));
                cursor = end;
            } else {
                ranges.push((start, content.len()));
                break;
            }
        }
    }
    ranges.sort_unstable();
    let mut merged = Vec::<(usize, usize)>::new();
    for (start, end) in ranges {
        if let Some((_, previous_end)) = merged.last_mut() {
            if start <= *previous_end {
                *previous_end = (*previous_end).max(end);
                continue;
            }
        }
        merged.push((start, end));
    }
    merged
}

fn next_unignored_markup(
    content: &str,
    needle: &str,
    mut cursor: usize,
    ignored_ranges: &[(usize, usize)],
) -> Option<usize> {
    while let Some(relative) = content[cursor..].find(needle) {
        let position = cursor + relative;
        if let Some((_, end)) = ignored_ranges
            .iter()
            .find(|(start, end)| position >= *start && position < *end)
        {
            cursor = *end;
        } else {
            return Some(position);
        }
    }
    None
}

fn mermaid_source_ranges(content: &str, ignored_ranges: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut cursor = 0;
    while let Some(open) = next_unignored_markup(content, "<mermaid>", cursor, ignored_ranges) {
        let body_start = open + "<mermaid>".len();
        if let Some(close) =
            next_unignored_markup(content, "</mermaid>", body_start, ignored_ranges)
        {
            let end = close + "</mermaid>".len();
            ranges.push((open, end));
            cursor = end;
        } else {
            ranges.push((open, content.len()));
            break;
        }
    }
    ranges
}

fn prose_counts_in_line(line: &str) -> ProseCounts {
    let visible = dokuwiki_visible_text(line);
    let mut counts = ProseCounts::default();
    let mut in_word = false;
    for ch in visible.chars() {
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

fn dokuwiki_visible_text(line: &str) -> String {
    let mut visible = String::new();
    let mut cursor = 0;
    while cursor < line.len() {
        let remainder = &line[cursor..];
        if let Some(link_text) = remainder.strip_prefix("[[") {
            if let Some(end) = link_text.find("]]") {
                let target = &link_text[..end];
                let label = target.split_once('|').map_or(target, |(_, label)| label);
                visible.push_str(label);
                cursor += end + 4;
                continue;
            }
        }
        if remainder.starts_with("''") || remainder.starts_with("%%") {
            let delimiter = &remainder[..2];
            if let Some(end) = remainder[2..].find(delimiter) {
                cursor += 2 + end + 2;
                continue;
            }
        }
        if remainder.starts_with("**")
            || remainder.starts_with("//")
            || remainder.starts_with("__")
            || remainder.starts_with("~~")
        {
            cursor += 2;
            continue;
        }
        if remainder.starts_with('<') {
            if let Some(end) = remainder.find('>') {
                let tag = &remainder[1..end];
                if tag
                    .chars()
                    .next()
                    .is_some_and(|first| first.is_ascii_alphabetic() || first == '/')
                {
                    cursor += end + 1;
                    continue;
                }
            }
        }
        let ch = remainder.chars().next().expect("cursor is within line");
        visible.push(ch);
        cursor += ch.len_utf8();
    }
    visible
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
