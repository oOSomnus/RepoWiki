use super::{collect_expected_pages, module_page_filename, validate_module_page_paths};
use crate::model::{Module, ModuleTree, Node};
use crate::session::{self, SessionState};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
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

pub(super) fn validate_mermaid_with_context(
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
pub(super) struct PageAssessmentContext<'a> {
    pub(super) is_leaf: bool,
    pub(super) is_overview: bool,
    pub(super) required_links: &'a [String],
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
pub(super) fn markdown_prose_word_count(content: &str) -> usize {
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
