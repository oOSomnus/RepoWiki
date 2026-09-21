use crate::language;
use crate::model::{
    ArtifactFile, ArtifactIndex, CallRelationship, Node, Summary, DEFAULT_CLUSTER_BATCH_SIZE,
    DEFAULT_MAX_TOKEN_PER_LEAF_MODULE, DEFAULT_MAX_TOKEN_PER_MODULE, SUPPORTED_LANGUAGES,
};
use crate::session::{self, SessionState};
use anyhow::{Context, Result};
use ignore::WalkBuilder;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::Path;
use std::process::Command;

const MAX_ARTIFACT_FILES_PER_CLASS: usize = 40;
const MAX_ARTIFACT_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Default)]
pub struct AnalyzeOptions {
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub focus: Option<String>,
    pub gitignore: bool,
    pub artifacts: bool,
    /// Maximum artifact budget expressed in approximate tokens. A zero value
    /// means use the engine default for direct library callers.
    pub artifact_token_budget: usize,
    pub artifact_exclude: Vec<String>,
    /// Analysis does not build the LLM tree, but this remains part of the
    /// summary contract and is passed through to the analyzer.
    pub max_depth: usize,
    /// Clustering limits are recorded in the analysis summary for the host
    /// agent and tree quality gate. Zero means use the engine defaults.
    pub max_token_per_module: usize,
    pub max_token_per_leaf_module: usize,
    pub with_prose: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisOutput {
    pub session_id: String,
    pub summary: Summary,
    pub graph_path: String,
    pub component_index_path: String,
    pub leaf_nodes_path: String,
    pub artifact_index_path: String,
    pub session_path: String,
}

pub fn analyze(
    repo_path: &Path,
    output_dir: &Path,
    options: &AnalyzeOptions,
    session_id: Option<&str>,
) -> Result<(SessionState, AnalysisOutput, BTreeMap<String, Node>)> {
    let repo_path = repo_path
        .canonicalize()
        .with_context(|| format!("repository does not exist: {}", repo_path.display()))?;
    let output_dir = if output_dir.is_absolute() {
        output_dir.to_path_buf()
    } else {
        repo_path.join(output_dir)
    };
    let _session_lock = session_id
        .map(|id| session::SessionLock::acquire(&repo_path, id))
        .transpose()?;
    fs::create_dir_all(&output_dir)?;
    let mut state = match session_id {
        Some(id) => session::load_unlocked(&repo_path, id)?,
        None => session::create(&repo_path, &output_dir)?,
    };
    if state.output_dir != output_dir.to_string_lossy() {
        state.output_dir = output_dir.to_string_lossy().into_owned();
    }

    let mut nodes = BTreeMap::new();
    let mut relationships = Vec::<CallRelationship>::new();
    let mut artifact_index = ArtifactIndex::default();
    let mut languages = BTreeSet::new();
    let mut file_count = 0usize;
    let extension_map = extension_map();
    let excluded = default_excluded_dirs();
    let artifact_budget = if options.artifact_token_budget == 0 {
        MAX_ARTIFACT_BYTES
    } else {
        (options.artifact_token_budget as u64).saturating_mul(4)
    };

    let mut builder = WalkBuilder::new(&repo_path);
    builder
        .standard_filters(true)
        .git_ignore(options.gitignore)
        .git_global(options.gitignore)
        .git_exclude(options.gitignore)
        .sort_by_file_name(|left, right| left.cmp(right));
    if options.gitignore {
        builder.add_custom_ignore_filename(".gitignore");
    }
    for result in builder.build() {
        let entry = match result {
            Ok(entry) => entry,
            Err(error) => {
                eprintln!("codewiki: skipped walk entry: {error}");
                continue;
            }
        };
        let path = entry.path();
        if !path.is_file() || path.starts_with(repo_path.join(".codewiki")) {
            continue;
        }
        let relative = match path.strip_prefix(&repo_path) {
            Ok(value) => value.to_string_lossy().replace('\\', "/"),
            Err(_) => continue,
        };
        if should_skip_path(&relative, &excluded, options) {
            continue;
        }
        let artifact = artifact_class(&relative);
        let artifact_enabled = options.artifacts
            && artifact.is_some()
            && !options
                .artifact_exclude
                .iter()
                .any(|pattern| matches_exclude_pattern(&relative, pattern));
        let language = path
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase)
            .and_then(|extension| extension_map.get(extension.as_str()).copied());
        if language.is_none() && !artifact_enabled {
            continue;
        }
        if language.is_some_and(|language| !SUPPORTED_LANGUAGES.contains(&language))
            && !artifact_enabled
        {
            continue;
        }
        let source = match fs::read_to_string(path) {
            Ok(source) => source,
            Err(_) => continue,
        };
        if artifact_enabled {
            register_artifact(
                &mut artifact_index,
                path,
                &relative,
                artifact.as_deref(),
                artifact_budget,
            )?;
            if artifact_index
                .files
                .get(&relative)
                .is_some_and(|file| file.included)
            {
                let artifact_nodes = extract_artifact_components(
                    &source,
                    &relative,
                    path,
                    artifact.as_deref().unwrap_or("config"),
                );
                if let Some(file) = artifact_index.files.get_mut(&relative) {
                    file.units = artifact_nodes
                        .iter()
                        .filter(|node| node.node_type.as_deref() == Some("artifact_unit"))
                        .map(|node| node.name.clone())
                        .collect();
                }
                for node in artifact_nodes {
                    nodes.insert(node.id.clone(), node);
                }
            }
        }
        if let Some(language) = language.filter(|language| SUPPORTED_LANGUAGES.contains(language)) {
            file_count += 1;
            languages.insert(language.to_string());
            match language::analyze_file(&source, &relative, path, language, artifact.as_deref()) {
                Ok(fragment) => {
                    for node in fragment.nodes {
                        nodes.insert(node.id.clone(), node);
                    }
                    relationships.extend(fragment.relationships);
                }
                Err(error) => {
                    eprintln!("codewiki: skipped {relative}: {error:#}");
                }
            }
        }
    }

    for paths in artifact_index.classes.values_mut() {
        paths.sort();
    }
    resolve_relationships(&mut nodes, relationships);
    let leaf_nodes = select_leaf_nodes(&nodes);
    let commit = git_head(&repo_path);
    let max_depth = if options.max_depth == 0 {
        2
    } else {
        options.max_depth
    };
    let max_token_per_module = if options.max_token_per_module == 0 {
        DEFAULT_MAX_TOKEN_PER_MODULE
    } else {
        options.max_token_per_module
    };
    let max_token_per_leaf_module = if options.max_token_per_leaf_module == 0 {
        DEFAULT_MAX_TOKEN_PER_LEAF_MODULE
    } else {
        options.max_token_per_leaf_module
    };
    let summary = Summary {
        repo_path: repo_path.to_string_lossy().into_owned(),
        output_dir: output_dir.to_string_lossy().into_owned(),
        total_components: nodes.len(),
        leaf_nodes: leaf_nodes.len(),
        max_depth,
        max_token_per_module,
        max_token_per_leaf_module,
        cluster_batch_size: DEFAULT_CLUSTER_BATCH_SIZE,
        supported_files: file_count,
        languages: languages.into_iter().collect(),
        analyzed_commit: commit,
        warnings: Vec::new(),
        documentation_profile: "architecture".to_string(),
    };

    let graph_dir = output_dir.join("temp").join("dependency_graphs");
    fs::create_dir_all(&graph_dir)?;
    let graph_path = graph_dir.join(format!(
        "{}_dependency_graph.json",
        sanitize_filename(
            repo_path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("repository")
        )
    ));
    if graph_path.exists() {
        let previous = graph_path.with_extension("json.prev");
        fs::copy(&graph_path, previous)?;
    }
    session::write_json(&graph_path, &nodes)?;
    let artifact_path = output_dir.join("temp").join("artifact_index.json");
    session::write_json(&artifact_path, &artifact_index)?;
    session::write_analysis_files(&mut state, &nodes, &leaf_nodes, &summary, &artifact_index)?;

    let output = AnalysisOutput {
        session_id: state.session_id.clone(),
        summary,
        graph_path: graph_path.to_string_lossy().into_owned(),
        component_index_path: session::session_value_path(&state, "component_index.json")
            .to_string_lossy()
            .into_owned(),
        leaf_nodes_path: session::session_value_path(&state, "leaf_nodes.json")
            .to_string_lossy()
            .into_owned(),
        artifact_index_path: artifact_path.to_string_lossy().into_owned(),
        session_path: session::session_root(&repo_path, &state.session_id)
            .to_string_lossy()
            .into_owned(),
    };
    Ok((state, output, nodes))
}

pub fn load_nodes_from_graph(path: &Path) -> Result<BTreeMap<String, Node>> {
    session::read_json(path)
}

/// Build a structural candidate tree from selected analysis leaves.
///
/// This is deliberately not presented as the final documentation tree.  It is
/// a deterministic starting point for the host agent's semantic clustering
/// pass; large candidates must still be recursively split with scope=module.
pub fn build_initial_module_tree(
    nodes: &BTreeMap<String, Node>,
    leaf_nodes: &[String],
) -> crate::model::ModuleTree {
    let mut tree = crate::model::ModuleTree::new();
    let selected = leaf_nodes
        .iter()
        .filter_map(|id| nodes.get(id))
        .collect::<Vec<_>>();
    for node in selected {
        let group = node
            .relative_path
            .split('/')
            .next()
            .filter(|part| !part.is_empty())
            .unwrap_or("Repository")
            .to_string();
        let entry = tree.entry(sanitize_filename(&group)).or_default();
        if entry.path.is_none() {
            entry.path = Some(
                Path::new(&node.relative_path)
                    .parent()
                    .map(|value| value.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_default(),
            );
        }
        entry.components.push(node.id.clone());
    }
    if tree.is_empty() {
        tree.insert(
            "Repository".to_string(),
            crate::model::Module {
                path: Some(".".to_string()),
                ..Default::default()
            },
        );
    }
    tree
}

fn extension_map() -> HashMap<&'static str, &'static str> {
    HashMap::from([
        ("py", "python"),
        ("pyx", "python"),
        ("java", "java"),
        ("js", "javascript"),
        ("jsx", "javascript"),
        ("mjs", "javascript"),
        ("ts", "typescript"),
        ("tsx", "typescript"),
        ("go", "go"),
        ("rs", "rust"),
        ("c", "c"),
        ("h", "c"),
        ("cc", "cpp"),
        ("cpp", "cpp"),
        ("cxx", "cpp"),
        ("c++", "cpp"),
        ("hpp", "cpp"),
        ("hxx", "cpp"),
        ("h++", "cpp"),
        ("cs", "csharp"),
        ("kt", "kotlin"),
        ("kts", "kotlin"),
        ("php", "php"),
        ("phtml", "php"),
        ("inc", "php"),
        ("rb", "ruby"),
        ("rake", "ruby"),
        ("scala", "scala"),
        ("sc", "scala"),
    ])
}

fn default_excluded_dirs() -> Vec<&'static str> {
    vec![
        ".git",
        ".codewiki",
        "target",
        "node_modules",
        ".venv",
        "venv",
        ".tox",
        "dist",
        "build",
        "vendor",
        "coverage",
        "__pycache__",
    ]
}

fn should_skip_path(relative: &str, excluded: &[&str], options: &AnalyzeOptions) -> bool {
    if excluded
        .iter()
        .any(|value| relative.split('/').any(|part| part == *value))
    {
        return true;
    }
    if options
        .exclude
        .iter()
        .any(|pattern| matches_exclude_pattern(relative, pattern))
    {
        return true;
    }
    if !options.include.is_empty()
        && !options
            .include
            .iter()
            .any(|pattern| matches_include_pattern(relative, pattern))
    {
        return true;
    }
    if let Some(focus) = &options.focus {
        if !relative.contains(focus) {
            return true;
        }
    }
    false
}

fn matches_include_pattern(relative: &str, pattern: &str) -> bool {
    let relative = normalize_glob_path(relative);
    let pattern = normalize_glob_path(pattern);
    let filename = relative.rsplit('/').next().unwrap_or(&relative);
    glob_match(&pattern, &relative) || glob_match(&pattern, filename)
}

fn matches_exclude_pattern(relative: &str, pattern: &str) -> bool {
    let relative = normalize_glob_path(relative);
    let pattern = normalize_glob_path(pattern);
    if pattern.is_empty() {
        return false;
    }
    let filename = relative.rsplit('/').next().unwrap_or(&relative);
    let directory_prefix = pattern.trim_end_matches('/');
    glob_match(&pattern, &relative)
        || glob_match(&pattern, filename)
        || (pattern.ends_with('/')
            && (relative == directory_prefix
                || relative.starts_with(&format!("{directory_prefix}/"))))
        || relative == pattern
        || relative.starts_with(&format!("{pattern}/"))
        || relative.split('/').any(|part| part == pattern)
}

fn normalize_glob_path(value: &str) -> String {
    value
        .replace('\\', "/")
        .trim_start_matches("./")
        .to_string()
}

fn glob_match(pattern: &str, value: &str) -> bool {
    let pattern = pattern.chars().collect::<Vec<_>>();
    let value = value.chars().collect::<Vec<_>>();
    let mut memo = HashMap::new();
    glob_match_at(&pattern, &value, 0, 0, &mut memo)
}

fn glob_match_at(
    pattern: &[char],
    value: &[char],
    pattern_index: usize,
    value_index: usize,
    memo: &mut HashMap<(usize, usize), bool>,
) -> bool {
    if let Some(result) = memo.get(&(pattern_index, value_index)) {
        return *result;
    }
    let result = if pattern_index == pattern.len() {
        value_index == value.len()
    } else {
        match pattern[pattern_index] {
            '*' => {
                glob_match_at(pattern, value, pattern_index + 1, value_index, memo)
                    || (value_index < value.len()
                        && glob_match_at(pattern, value, pattern_index, value_index + 1, memo))
            }
            '?' => {
                value_index < value.len()
                    && glob_match_at(pattern, value, pattern_index + 1, value_index + 1, memo)
            }
            '[' => match_glob_class(pattern, pattern_index, value.get(value_index).copied())
                .map(|(next_pattern_index, matched)| {
                    matched
                        && glob_match_at(pattern, value, next_pattern_index, value_index + 1, memo)
                })
                .unwrap_or_else(|| {
                    value.get(value_index) == Some(&'[')
                        && glob_match_at(pattern, value, pattern_index + 1, value_index + 1, memo)
                }),
            literal => {
                value.get(value_index) == Some(&literal)
                    && glob_match_at(pattern, value, pattern_index + 1, value_index + 1, memo)
            }
        }
    };
    memo.insert((pattern_index, value_index), result);
    result
}

fn match_glob_class(pattern: &[char], start: usize, value: Option<char>) -> Option<(usize, bool)> {
    let mut index = start + 1;
    if index >= pattern.len() {
        return None;
    }
    let negated = matches!(pattern[index], '!' | '^');
    if negated {
        index += 1;
    }
    if index >= pattern.len() {
        return None;
    }

    let mut matched = false;
    let mut found_end = false;
    while index < pattern.len() {
        if pattern[index] == ']' {
            found_end = true;
            index += 1;
            break;
        }
        let first = pattern[index];
        index += 1;
        if index + 1 < pattern.len() && pattern[index] == '-' && pattern[index + 1] != ']' {
            let last = pattern[index + 1];
            index += 2;
            if let Some(value) = value {
                matched |= first <= value && value <= last;
            }
        } else if Some(first) == value {
            matched = true;
        }
    }
    if !found_end {
        return None;
    }
    Some((index, if negated { !matched } else { matched }))
}

fn add_resolution_name(index: &mut HashMap<String, Vec<String>>, key: &str, id: &str) {
    if key.is_empty() {
        return;
    }
    let values = index.entry(key.to_string()).or_default();
    if !values.iter().any(|value| value == id) {
        values.push(id.to_string());
    }
}

fn add_resolution_language(
    index: &mut HashMap<(String, String), Vec<String>>,
    language: &str,
    key: &str,
    id: &str,
) {
    if key.is_empty() {
        return;
    }
    let values = index
        .entry((language.to_string(), key.to_string()))
        .or_default();
    if !values.iter().any(|value| value == id) {
        values.push(id.to_string());
    }
}

fn resolve_relationships(nodes: &mut BTreeMap<String, Node>, relationships: Vec<CallRelationship>) {
    let mut exact: HashMap<String, Vec<String>> = HashMap::new();
    let mut simple: HashMap<String, Vec<String>> = HashMap::new();
    let mut by_language: HashMap<(String, String), Vec<String>> = HashMap::new();

    for node in nodes.values() {
        for name in [
            node.id.as_str(),
            node.component_id.as_deref().unwrap_or_default(),
            node.name.as_str(),
            node.qualified_name.as_str(),
        ] {
            add_resolution_name(&mut exact, name, &node.id);
            add_resolution_language(&mut by_language, &node.language, name, &node.id);
            let simple_name = name.rsplit(['.', ':']).next().unwrap_or(name);
            add_resolution_name(&mut simple, simple_name, &node.id);
            add_resolution_language(&mut by_language, &node.language, simple_name, &node.id);
        }
    }

    for values in exact.values_mut() {
        values.sort();
    }
    for values in simple.values_mut() {
        values.sort();
    }
    for values in by_language.values_mut() {
        values.sort();
    }

    let mut dependencies: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for relationship in relationships {
        let Some(caller) = nodes.get(&relationship.caller) else {
            continue;
        };
        let resolved = if relationship.is_resolved && nodes.contains_key(&relationship.callee) {
            Some(relationship.callee.clone())
        } else {
            resolve_relationship_target(
                &relationship.callee,
                &caller.id,
                &caller.language,
                nodes,
                &exact,
                &simple,
                &by_language,
            )
        };
        if let Some(target) = resolved {
            if target != caller.id {
                dependencies
                    .entry(caller.id.clone())
                    .or_default()
                    .insert(target);
            }
        }
    }

    for node in nodes.values_mut() {
        node.depends_on = dependencies
            .remove(&node.id)
            .unwrap_or_default()
            .into_iter()
            .collect();
    }
}

fn resolve_relationship_target(
    target: &str,
    caller_id: &str,
    caller_language: &str,
    nodes: &BTreeMap<String, Node>,
    exact: &HashMap<String, Vec<String>>,
    simple: &HashMap<String, Vec<String>>,
    by_language: &HashMap<(String, String), Vec<String>>,
) -> Option<String> {
    let unique = |values: Option<&Vec<String>>| {
        let mut matches = values
            .into_iter()
            .flatten()
            .filter(|candidate| candidate.as_str() != caller_id);
        let first = matches.next()?.clone();
        matches.next().is_none().then_some(first)
    };
    let language_unique =
        |key: &str| unique(by_language.get(&(caller_language.to_string(), key.to_string())));

    if let Some(candidate) = language_unique(target).or_else(|| unique(exact.get(target))) {
        return nodes.contains_key(&candidate).then_some(candidate);
    }

    let suffixes = [
        target.rsplit("::").next().unwrap_or(target),
        target.rsplit('.').next().unwrap_or(target),
    ];
    for suffix in suffixes {
        if let Some(candidate) = language_unique(suffix)
            .or_else(|| unique(simple.get(suffix)))
            .or_else(|| unique(exact.get(suffix)))
        {
            if nodes.contains_key(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

/// Match the reference leaf-selection contract.
///
/// The clustering prompt receives architecture-bearing components, not every
/// function that happens to have no outgoing edge.  Classes/interfaces/structs
/// are always candidates; free functions are candidates when a repository is
/// function-dominated.  Artifact files and units are always candidates so
/// build/deploy/configuration behaviour cannot disappear from the wiki.
fn select_leaf_nodes(nodes: &BTreeMap<String, Node>) -> Vec<String> {
    const LEAF_REDUCTION_THRESHOLD: usize = 400;
    const OOP_TYPES: &[&str] = &[
        "class",
        "record",
        "interface",
        "struct",
        "enum",
        "trait",
        "union",
        "module",
        "object",
        "namespace",
        "type",
    ];

    let oop_count = nodes
        .values()
        .filter(|node| OOP_TYPES.contains(&node.component_type.as_str()))
        .count();
    let function_count = nodes
        .values()
        .filter(|node| node.component_type == "function")
        .count();
    let include_functions = oop_count == 0
        || (oop_count < 20 && function_count > oop_count)
        || (function_count > 0 && oop_count as f64 / ((oop_count + function_count) as f64) < 0.2);

    let mut candidates = nodes
        .values()
        .filter(|node| {
            node.component_type == "artifact"
                || OOP_TYPES.contains(&node.component_type.as_str())
                || (include_functions && node.component_type == "function")
        })
        .map(|node| node.id.clone())
        .collect::<BTreeSet<_>>();

    if candidates.len() >= LEAF_REDUCTION_THRESHOLD {
        // Artifact references describe build/deployment inputs; they must not
        // demote the code component they point at from a documentation leaf.
        for node in nodes
            .values()
            .filter(|node| node.component_type != "artifact")
        {
            for dependency in &node.depends_on {
                candidates.remove(dependency);
            }
        }
    }

    candidates.into_iter().collect()
}

fn artifact_class(relative: &str) -> Option<String> {
    let lower = relative.to_ascii_lowercase();
    let file = lower.rsplit('/').next().unwrap_or(&lower);
    if file == "dockerfile" || file.starts_with("dockerfile.") || lower.contains("/docker/") {
        return Some("container".to_string());
    }
    if lower.ends_with(".github/workflows")
        || lower.contains(".github/workflows/")
        || lower.contains("gitlab-ci")
    {
        return Some("ci".to_string());
    }
    if [
        "cargo.toml",
        "go.mod",
        "pyproject.toml",
        "package.json",
        "pom.xml",
        "build.gradle",
        "setup.py",
    ]
    .contains(&file)
    {
        return Some("manifest".to_string());
    }
    if [
        "makefile",
        "justfile",
        "rakefile",
        "build.gradle",
        "gradlew",
        "setup.cfg",
    ]
    .contains(&file)
    {
        return Some("build".to_string());
    }
    if lower.contains("/schema") || lower.contains("/migrations/") || lower.ends_with(".sql") {
        return Some("schema".to_string());
    }
    if lower.contains("/config") || file.ends_with(".env.example") || file.starts_with("config.") {
        return Some("config".to_string());
    }
    if lower.contains("/scripts/") || lower.contains("/bin/") || lower.ends_with(".sh") {
        return Some("script".to_string());
    }
    if lower.contains("/test") || lower.contains("/fixtures/") {
        return Some("test_infra".to_string());
    }
    if [
        "cargo.lock",
        "go.sum",
        "requirements.txt",
        "poetry.lock",
        "package-lock.json",
        "yarn.lock",
        "pnpm-lock.yaml",
    ]
    .contains(&file)
    {
        return Some("packaging".to_string());
    }
    None
}

fn extract_artifact_components(
    source: &str,
    relative: &str,
    path: &Path,
    class: &str,
) -> Vec<Node> {
    let file_name = relative.rsplit('/').next().unwrap_or(relative);
    let file_id = format!("{relative}::{file_name}");
    let line_count = source.lines().count().max(1);
    let mut nodes = vec![Node {
        id: file_id.clone(),
        name: file_name.to_string(),
        component_type: "artifact".to_string(),
        file_path: path.to_string_lossy().into_owned(),
        relative_path: relative.to_string(),
        source_code: source.to_string(),
        start_line: 1,
        end_line: line_count,
        node_type: Some("artifact_file".to_string()),
        display_name: Some(relative.to_string()),
        component_id: Some(file_id.clone()),
        language: "Artifact".to_string(),
        qualified_name: file_id,
        artifact_class: Some(class.to_string()),
        ..Default::default()
    }];

    for (name, start_line, end_line, unit_source) in artifact_unit_specs(source, file_name, class) {
        if name == file_name {
            continue;
        }
        let id = format!("{relative}::{name}");
        nodes.push(Node {
            id: id.clone(),
            name: name.clone(),
            component_type: "artifact".to_string(),
            file_path: path.to_string_lossy().into_owned(),
            relative_path: relative.to_string(),
            source_code: unit_source,
            start_line,
            end_line,
            node_type: Some("artifact_unit".to_string()),
            display_name: Some(format!("{relative}::{name}")),
            component_id: Some(id.clone()),
            language: "Artifact".to_string(),
            qualified_name: id,
            artifact_class: Some(class.to_string()),
            ..Default::default()
        });
    }
    nodes
}

fn artifact_unit_specs(
    source: &str,
    file_name: &str,
    class: &str,
) -> Vec<(String, usize, usize, String)> {
    let mut units = Vec::new();
    if file_name == "package.json" {
        if let Ok(Value::Object(root)) = serde_json::from_str::<Value>(source) {
            if let Some(Value::Object(scripts)) = root.get("scripts") {
                for (name, command) in scripts {
                    let command = command.as_str().unwrap_or_default();
                    units.push((name.clone(), 1, 1, format!("\"{name}\": {command}")));
                }
            }
        }
    } else if file_name.eq_ignore_ascii_case("makefile")
        || file_name.eq_ignore_ascii_case("gnumakefile")
        || file_name.ends_with(".mk")
    {
        let target_re = Regex::new(r"(?m)^([A-Za-z0-9_.-]+)\s*:").expect("make target regex");
        for captures in target_re.captures_iter(source) {
            let Some(name) = captures.get(1).map(|value| value.as_str().to_string()) else {
                continue;
            };
            let start = source[..captures.get(0).expect("target match").start()]
                .lines()
                .count()
                .max(1);
            units.push((name, start, start, captures[0].to_string()));
        }
    } else if class == "container" && file_name.to_ascii_lowercase().starts_with("dockerfile") {
        let stage_re = Regex::new(r"(?im)^\s*FROM\s+[^\n]+(?:\s+AS\s+([A-Za-z0-9_.-]+))?")
            .expect("docker stage regex");
        let mut stage_number = 1;
        for captures in stage_re.captures_iter(source) {
            let name = captures
                .get(1)
                .map(|value| value.as_str().to_string())
                .unwrap_or_else(|| {
                    let name = format!("stage_{stage_number}");
                    stage_number += 1;
                    name
                });
            let start = source[..captures.get(0).expect("stage match").start()]
                .lines()
                .count()
                .max(1);
            units.push((name, start, start, captures[0].to_string()));
        }
    } else if class == "ci" {
        let job_re = Regex::new(r"(?m)^\s{2}([A-Za-z0-9_.-]+):\s*$").expect("CI job regex");
        for captures in job_re.captures_iter(source) {
            let name = captures[1].to_string();
            let start = source[..captures.get(0).expect("job match").start()]
                .lines()
                .count()
                .max(1);
            units.push((name, start, start, captures[0].to_string()));
        }
    }
    units.sort_by(|left, right| left.0.cmp(&right.0));
    units.dedup_by(|left, right| left.0 == right.0);
    units.truncate(25);
    units
}

fn register_artifact(
    index: &mut ArtifactIndex,
    path: &Path,
    relative: &str,
    class: Option<&str>,
    max_bytes: u64,
) -> Result<()> {
    let Some(class) = class else {
        return Ok(());
    };
    let size = fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    let entries = index.classes.entry(class.to_string()).or_default();
    let included = entries.len() < MAX_ARTIFACT_FILES_PER_CLASS
        && index.total_bytes.saturating_add(size) <= max_bytes;
    index.files.insert(
        relative.to_string(),
        ArtifactFile {
            path: relative.to_string(),
            class: class.to_string(),
            size_bytes: size,
            included,
            units: Vec::new(),
        },
    );
    if included {
        entries.push(relative.to_string());
        index.total_bytes = index.total_bytes.saturating_add(size);
    }
    Ok(())
}

fn sanitize_filename(value: &str) -> String {
    let mut result = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            result.push(ch);
        } else {
            result.push('_');
        }
    }
    if result.is_empty() {
        "repository".to_string()
    } else {
        result
    }
}

fn git_head(repo_path: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["-C", repo_path.to_str()?, "rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str, relative: &str, language: &str) -> language::FileAnalysis {
        language::analyze_file(source, relative, Path::new(relative), language, None)
            .expect("Tree-sitter fixture should parse")
    }

    #[test]
    fn extracts_components_for_python() {
        let fragment = parse(
            "class Service:\n    def run(self):\n        return helper()\n\ndef helper():\n    return 1\n",
            "service.py",
            "python",
        );
        assert_eq!(fragment.nodes.len(), 3);
        assert!(fragment.nodes.iter().any(|node| node.name == "Service"));
        assert!(fragment.nodes.iter().any(|node| node.name == "Service.run"));
        assert!(fragment.nodes.iter().any(|node| node.name == "helper"));
        assert!(fragment
            .relationships
            .iter()
            .any(|relationship| relationship.callee == "service.py::helper"));
    }

    #[test]
    fn ast_relationships_ignore_comments_and_strings() {
        let fragment = parse(
            "fn outer() -> i32 {\n    let text = r#\"helper()\"#;\n    // helper()\n    helper()\n}\nfn helper() -> i32 { 1 }\n",
            "service.rs",
            "rust",
        );
        let mut indexed = fragment
            .nodes
            .into_iter()
            .map(|node| (node.id.clone(), node))
            .collect::<BTreeMap<_, _>>();
        resolve_relationships(&mut indexed, fragment.relationships);
        assert_eq!(
            indexed["service.rs::outer"].depends_on,
            vec!["service.rs::helper".to_string()]
        );
    }

    #[test]
    fn ambiguous_same_language_dependencies_are_not_expanded() {
        let sources = [
            ("caller.rs", "fn caller() -> i32 { helper() + unique() }\n"),
            (
                "first.rs",
                "fn helper() -> i32 { 1 }\nfn unique() -> i32 { 2 }\n",
            ),
            ("second.rs", "fn helper() -> i32 { 3 }\n"),
        ];
        let mut indexed = BTreeMap::new();
        let mut relationships = Vec::new();
        for (relative, source) in sources {
            let fragment = parse(source, relative, "rust");
            for node in fragment.nodes {
                indexed.insert(node.id.clone(), node);
            }
            relationships.extend(fragment.relationships);
        }

        resolve_relationships(&mut indexed, relationships);

        assert_eq!(
            indexed["caller.rs::caller"].depends_on,
            vec!["first.rs::unique".to_string()]
        );
    }

    #[test]
    fn ast_does_not_promote_expression_calls_to_components() {
        let rust_fragment = parse(
            "struct State { aggregated_output: Option<String> }\nfn build() -> State {\n    let state = State { aggregated_output: Some(\"value\") };\n    state\n}\n",
            "state.rs",
            "rust",
        );
        assert!(rust_fragment.nodes.iter().any(|node| node.name == "State"));
        assert!(rust_fragment.nodes.iter().any(|node| node.name == "build"));
        assert!(!rust_fragment.nodes.iter().any(|node| node.name == "Some"));

        let go_fragment = parse(
            "type State struct { model string }\nfunc build() State { return State{model: makeModel()} }\n",
            "state.go",
            "go",
        );
        assert!(go_fragment.nodes.iter().any(|node| node.name == "build"));
        assert!(!go_fragment
            .nodes
            .iter()
            .any(|node| node.name == "makeModel"));

        let javascript_fragment = parse(
            "const state = { model: makeModel() };\n",
            "state.js",
            "javascript",
        );
        assert!(!javascript_fragment
            .nodes
            .iter()
            .any(|node| node.name == "makeModel"));
    }

    #[test]
    fn artifact_classifies_manifests() {
        assert_eq!(artifact_class("package.json"), Some("manifest".to_string()));
        assert_eq!(artifact_class("go.mod"), Some("manifest".to_string()));
        assert_eq!(artifact_class("go.sum"), Some("packaging".to_string()));
        assert_eq!(
            artifact_class(".github/workflows/test.yml"),
            Some("ci".to_string())
        );
    }

    #[test]
    fn initial_module_tree_contains_only_selected_analysis_leaves() {
        let mut nodes = BTreeMap::new();
        nodes.insert(
            "src/service.rs::Service".to_string(),
            Node {
                id: "src/service.rs::Service".to_string(),
                relative_path: "src/service.rs".to_string(),
                ..Node::default()
            },
        );
        nodes.insert(
            "src/service.rs::Service.run".to_string(),
            Node {
                id: "src/service.rs::Service.run".to_string(),
                relative_path: "src/service.rs".to_string(),
                ..Node::default()
            },
        );

        let tree = build_initial_module_tree(&nodes, &["src/service.rs::Service".to_string()]);
        assert_eq!(tree["src"].components, vec!["src/service.rs::Service"]);
    }
}
