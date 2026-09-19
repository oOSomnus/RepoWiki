use crate::model::{ArtifactFile, ArtifactIndex, Node, Summary, SUPPORTED_LANGUAGES};
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
    /// means use the engine default, keeping direct library callers compatible.
    pub artifact_token_budget: usize,
    pub artifact_exclude: Vec<String>,
    /// Analysis does not build the LLM tree, but this remains part of the
    /// summary contract and is passed through to the analyzer.
    pub max_depth: usize,
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
    fs::create_dir_all(&output_dir)?;
    let mut state = match session_id {
        Some(id) => session::load(&repo_path, id)?,
        None => session::create(&repo_path, &output_dir)?,
    };
    if state.output_dir != output_dir.to_string_lossy() {
        state.output_dir = output_dir.to_string_lossy().into_owned();
    }

    let mut nodes = BTreeMap::new();
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
        let language = match path.extension().and_then(|value| value.to_str()) {
            Some(extension) => extension_map.get(extension).copied(),
            None => None,
        };
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
            let components =
                extract_components(&source, &relative, path, language, artifact.as_deref());
            for node in components {
                nodes.insert(node.id.clone(), node);
            }
        }
    }

    for paths in artifact_index.classes.values_mut() {
        paths.sort();
    }
    resolve_dependencies(&mut nodes);
    let leaf_nodes = select_leaf_nodes(&nodes);
    let commit = git_head(&repo_path);
    let summary = Summary {
        repo_path: repo_path.to_string_lossy().into_owned(),
        output_dir: output_dir.to_string_lossy().into_owned(),
        total_components: nodes.len(),
        leaf_nodes: leaf_nodes.len(),
        max_depth: if options.max_depth == 0 {
            2
        } else {
            options.max_depth
        },
        supported_files: file_count,
        languages: languages.into_iter().collect(),
        analyzed_commit: commit,
        warnings: Vec::new(),
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

pub fn build_initial_module_tree(nodes: &BTreeMap<String, Node>) -> crate::model::ModuleTree {
    let mut tree = crate::model::ModuleTree::new();
    for node in nodes.values() {
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
        ("py", "Python"),
        ("java", "Java"),
        ("js", "JavaScript"),
        ("jsx", "JavaScript"),
        ("mjs", "JavaScript"),
        ("ts", "TypeScript"),
        ("tsx", "TypeScript"),
        ("c", "C"),
        ("h", "C"),
        ("cc", "C++"),
        ("cpp", "C++"),
        ("cxx", "C++"),
        ("hpp", "C++"),
        ("cs", "C#"),
        ("kt", "Kotlin"),
        ("kts", "Kotlin"),
        ("php", "PHP"),
        ("rb", "Ruby"),
        ("rake", "Ruby"),
        ("scala", "Scala"),
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

#[derive(Debug, Clone)]
struct Candidate {
    start: usize,
    name: String,
    component_type: String,
    parameters: Vec<String>,
    class_path: Vec<String>,
}

#[derive(Debug, Clone)]
struct ClassScope {
    path: Vec<String>,
    indent: usize,
    open_depth: usize,
    pending_brace: bool,
}

fn extract_components(
    source: &str,
    relative: &str,
    path: &Path,
    language: &str,
    artifact: Option<&str>,
) -> Vec<Node> {
    let class_re = Regex::new(
        r"(?i)^(?:export\s+)?(?:default\s+)?(?:(?:public|private|protected|internal|final|abstract|sealed)\s+)*(class|interface|struct|enum|trait|object|module)\s+([A-Za-z_][A-Za-z0-9_]*)",
    )
    .expect("class regex");
    let function_re = Regex::new(
        r"(?i)^(?:async\s+)?(?:export\s+)?(?:public\s+|private\s+|protected\s+|static\s+|final\s+|suspend\s+|internal\s+)*\b(def|function|fn|fun|func|proc|method)\s+([A-Za-z_][A-Za-z0-9_!?]*)\s*(?:<[^>]*>)?\s*(?:\(([^)]*)\))?",
    )
    .expect("function regex");
    let declaration_re = Regex::new(
        r"(?i)^(?:export\s+)?(?:const|let|var)\s+([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(?:async\s*)?(?:\([^)]*\)|[A-Za-z_][A-Za-z0-9_]*)\s*=>",
    )
    .expect("declaration regex");
    let generic_re = Regex::new(
        r"(?i)^(?:(?:public|private|protected|internal|static|final|virtual|override|async|inline|constexpr|extern|suspend|abstract)\s+)*(?:[A-Za-z_][A-Za-z0-9_:.<>,\[\]*&?]*\s+)+([A-Za-z_][A-Za-z0-9_]*)\s*\(([^)]*)\)",
    )
    .expect("generic function regex");
    let method_re = Regex::new(r"^([A-Za-z_][A-Za-z0-9_]*)\s*\(([^)]*)\)").expect("method regex");
    let python_re = Regex::new(r"^(?:async\s+)?def\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(([^)]*)\)")
        .expect("python regex");
    let ruby_re =
        Regex::new(r"^def\s+([A-Za-z_][A-Za-z0-9_!?=]*)(?:\(([^)]*)\))?").expect("ruby regex");
    let lines: Vec<&str> = source.lines().collect();
    let code_source = strip_comments_and_strings(source);
    let code_lines: Vec<&str> = code_source.lines().collect();
    let indentation_scoped = matches!(language, "Python" | "Ruby");
    let mut class_stack: Vec<ClassScope> = Vec::new();
    let mut brace_depth = 0usize;
    let mut candidates = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let code_line = code_lines.get(index).copied().unwrap_or_default();
        let code_trimmed = code_line.trim_start();
        if code_trimmed.is_empty() {
            continue;
        }

        if indentation_scoped {
            let indent = indentation_width(line);
            while class_stack
                .last()
                .map(|scope| indent <= scope.indent)
                .unwrap_or(false)
            {
                class_stack.pop();
            }
        } else {
            while class_stack
                .last()
                .map(|scope| !scope.pending_brace && brace_depth <= scope.open_depth)
                .unwrap_or(false)
            {
                class_stack.pop();
            }
            if let Some(scope) = class_stack.last_mut() {
                if scope.pending_brace && code_line.contains('{') {
                    scope.pending_brace = false;
                    scope.open_depth = brace_depth;
                }
            }
        }

        let containing_class = class_stack
            .last()
            .map(|scope| scope.path.clone())
            .unwrap_or_default();
        if let Some(caps) = class_re.captures(code_trimmed) {
            let class_name = caps[2].to_string();
            let mut class_path = containing_class.clone();
            class_path.push(class_name.clone());
            candidates.push(Candidate {
                start: index,
                name: class_name.clone(),
                component_type: caps[1].to_lowercase(),
                parameters: Vec::new(),
                class_path: containing_class,
            });
            class_stack.push(ClassScope {
                path: class_path,
                indent: indentation_width(line),
                open_depth: brace_depth,
                pending_brace: !indentation_scoped && !code_line.contains('{'),
            });
            brace_depth = update_brace_depth(brace_depth, code_line);
            continue;
        }

        let function_match = if language == "Python" {
            python_re.captures(code_trimmed)
        } else if language == "Ruby" {
            ruby_re.captures(code_trimmed)
        } else {
            function_re.captures(code_trimmed)
        };
        if let Some(caps) = function_match {
            let name_index = if language == "Python" || language == "Ruby" {
                1
            } else {
                2
            };
            let params_index = if language == "Python" || language == "Ruby" {
                2
            } else {
                3
            };
            let params = caps
                .get(params_index)
                .map(|value| {
                    value
                        .as_str()
                        .split(',')
                        .map(|item| item.trim().to_string())
                        .filter(|item| !item.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            candidates.push(Candidate {
                start: index,
                name: caps[name_index].to_string(),
                component_type: if containing_class.is_empty() {
                    "function".to_string()
                } else {
                    "method".to_string()
                },
                parameters: params,
                class_path: containing_class,
            });
            brace_depth = update_brace_depth(brace_depth, code_line);
            continue;
        }
        if let Some(caps) = declaration_re.captures(code_trimmed) {
            candidates.push(Candidate {
                start: index,
                name: caps[1].to_string(),
                component_type: if containing_class.is_empty() {
                    "function".to_string()
                } else {
                    "method".to_string()
                },
                parameters: Vec::new(),
                class_path: containing_class,
            });
            brace_depth = update_brace_depth(brace_depth, code_line);
            continue;
        }
        if !starts_with_control_statement(code_trimmed) {
            if let Some(caps) = generic_re.captures(code_trimmed) {
                let name = caps[1].to_string();
                if !is_control_call(&name) {
                    candidates.push(Candidate {
                        start: index,
                        name,
                        component_type: if containing_class.is_empty() {
                            "function".to_string()
                        } else {
                            "method".to_string()
                        },
                        parameters: caps[2]
                            .split(',')
                            .map(str::trim)
                            .filter(|item| !item.is_empty())
                            .map(str::to_string)
                            .collect(),
                        class_path: containing_class.clone(),
                    });
                    brace_depth = update_brace_depth(brace_depth, code_line);
                    continue;
                }
            }
        }
        if matches!(language, "JavaScript" | "TypeScript") {
            if let Some(caps) = method_re.captures(code_trimmed) {
                let name = caps[1].to_string();
                if !is_control_call(&name) {
                    candidates.push(Candidate {
                        start: index,
                        name,
                        component_type: if containing_class.is_empty() {
                            "function".to_string()
                        } else {
                            "method".to_string()
                        },
                        parameters: caps[2]
                            .split(',')
                            .map(str::trim)
                            .filter(|item| !item.is_empty())
                            .map(str::to_string)
                            .collect(),
                        class_path: containing_class,
                    });
                }
            }
        }
        brace_depth = update_brace_depth(brace_depth, code_line);
    }
    if candidates.is_empty() && !source.trim().is_empty() {
        candidates.push(Candidate {
            start: 0,
            name: file_component_name(relative),
            component_type: "file".to_string(),
            parameters: Vec::new(),
            class_path: Vec::new(),
        });
    }
    let mut counts: HashMap<String, usize> = HashMap::new();
    for candidate in &candidates {
        let name = qualified_candidate_name(candidate);
        *counts.entry(name).or_default() += 1;
    }
    let candidate_starts = candidates
        .iter()
        .map(|candidate| candidate.start)
        .collect::<Vec<_>>();
    candidates
        .into_iter()
        .enumerate()
        .map(|(candidate_index, candidate)| {
            let start = candidate.start;
            let actual_end = candidate_starts
                .get(candidate_index + 1)
                .copied()
                .unwrap_or(lines.len());
            let actual_end = actual_end.max(start + 1).min(lines.len());
            let source_code = lines[start..actual_end].join("\n");
            let qualified_name = qualified_candidate_name(&candidate);
            let base_id = format!("{}::{}", relative, qualified_name);
            let id_suffix = if counts.get(&qualified_name).copied().unwrap_or(0) > 1 {
                format!("#{}", start + 1)
            } else {
                String::new()
            };
            let id = format!("{}{}", base_id, id_suffix);
            let class_name = if candidate.class_path.is_empty() {
                None
            } else {
                Some(candidate.class_path.join("."))
            };
            let component_type = candidate.component_type;
            let display_name = if component_type == "method" {
                format!("method {}", qualified_name)
            } else {
                format!("{} {}", component_type, qualified_name)
            };
            let docstring = first_docstring(&source_code, language);
            Node {
                id: id.clone(),
                name: qualified_name.clone(),
                component_type: component_type.clone(),
                file_path: path.to_string_lossy().into_owned(),
                relative_path: relative.to_string(),
                depends_on: Vec::new(),
                source_code,
                start_line: start + 1,
                end_line: actual_end,
                has_docstring: docstring.is_some(),
                docstring,
                parameters: candidate.parameters,
                node_type: Some(component_type),
                base_classes: Vec::new(),
                class_name,
                display_name: Some(display_name),
                component_id: Some(id.clone()),
                language: language.to_string(),
                qualified_name: id,
                artifact_class: artifact.map(str::to_string),
            }
        })
        .collect()
}

fn qualified_candidate_name(candidate: &Candidate) -> String {
    if candidate.class_path.is_empty() {
        candidate.name.clone()
    } else {
        format!("{}.{}", candidate.class_path.join("."), candidate.name)
    }
}

fn indentation_width(line: &str) -> usize {
    line.chars()
        .take_while(|character| matches!(character, ' ' | '\t'))
        .map(|character| if character == '\t' { 4 } else { 1 })
        .sum()
}

fn update_brace_depth(depth: usize, line: &str) -> usize {
    let opens = line.chars().filter(|character| *character == '{').count();
    let closes = line.chars().filter(|character| *character == '}').count();
    depth.saturating_add(opens).saturating_sub(closes)
}

fn is_control_call(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "if" | "for" | "while" | "switch" | "catch" | "return" | "sizeof"
    )
}

fn starts_with_control_statement(line: &str) -> bool {
    matches!(
        line.split_whitespace()
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "if" | "for" | "while" | "switch" | "catch" | "return" | "throw" | "new"
    )
}

fn first_docstring(source: &str, language: &str) -> Option<String> {
    let lines: Vec<&str> = source.lines().collect();
    for line in lines.iter().skip(1).take(3) {
        let trimmed = line.trim();
        let candidate = if language == "Python" {
            trimmed.trim_matches('"').trim_matches('\'')
        } else if trimmed.starts_with("//") {
            trimmed.trim_start_matches('/').trim()
        } else {
            continue;
        };
        if !candidate.is_empty() {
            return Some(candidate.to_string());
        }
    }
    None
}

/*
 * The analyzer intentionally remains parser-light, but dependency edges must
 * be based on code tokens rather than prose. Keep line breaks while replacing
 * comments and literals with spaces so line spans and subsequent tokenization
 * remain stable.
 */
fn strip_comments_and_strings(source: &str) -> String {
    let chars = source.chars().collect::<Vec<_>>();
    let mut output = String::with_capacity(source.len());
    let mut index = 0usize;
    let mut line_comment = false;
    let mut block_comment = false;
    let mut quote: Option<(char, bool)> = None;

    while index < chars.len() {
        let character = chars[index];
        if line_comment {
            if character == '\n' {
                line_comment = false;
                output.push('\n');
            } else {
                output.push(' ');
            }
            index += 1;
            continue;
        }
        if block_comment {
            if character == '*' && chars.get(index + 1) == Some(&'/') {
                output.push(' ');
                output.push(' ');
                index += 2;
                block_comment = false;
            } else if character == '\n' {
                output.push('\n');
                index += 1;
            } else {
                output.push(' ');
                index += 1;
            }
            continue;
        }
        if let Some((delimiter, triple)) = quote {
            if triple
                && character == delimiter
                && chars.get(index + 1) == Some(&delimiter)
                && chars.get(index + 2) == Some(&delimiter)
            {
                output.extend([' ', ' ', ' ']);
                index += 3;
                quote = None;
            } else if !triple && character == delimiter {
                output.push(' ');
                index += 1;
                quote = None;
            } else if !triple && character == '\\' {
                output.push(' ');
                index += 1;
                if let Some(escaped) = chars.get(index) {
                    output.push(if *escaped == '\n' { '\n' } else { ' ' });
                    index += 1;
                }
            } else if character == '\n' {
                output.push('\n');
                index += 1;
            } else {
                output.push(' ');
                index += 1;
            }
            continue;
        }

        if character == '/' && chars.get(index + 1) == Some(&'/') {
            output.push(' ');
            output.push(' ');
            index += 2;
            line_comment = true;
        } else if character == '/' && chars.get(index + 1) == Some(&'*') {
            output.push(' ');
            output.push(' ');
            index += 2;
            block_comment = true;
        } else if character == '#' {
            output.push(' ');
            index += 1;
            line_comment = true;
        } else if matches!(character, '\'' | '"' | '`') {
            let triple = character != '`'
                && chars.get(index + 1) == Some(&character)
                && chars.get(index + 2) == Some(&character);
            output.push(' ');
            index += 1;
            if triple {
                output.extend([' ', ' ']);
                index += 2;
            }
            quote = Some((character, triple));
        } else {
            output.push(character);
            index += 1;
        }
    }
    output
}

fn resolve_dependencies(nodes: &mut BTreeMap<String, Node>) {
    let mut by_name: HashMap<String, Vec<String>> = HashMap::new();
    for node in nodes.values() {
        let mut names = vec![node.name.clone()];
        if let Some(simple_name) = node.name.rsplit('.').next() {
            if simple_name != node.name {
                names.push(simple_name.to_string());
            }
        }
        for name in names {
            let candidates = by_name.entry(name).or_default();
            if !candidates.contains(&node.id) {
                candidates.push(node.id.clone());
            }
        }
    }
    for candidates in by_name.values_mut() {
        candidates.sort();
    }
    let token_re = Regex::new(r"[A-Za-z_][A-Za-z0-9_!?]*").expect("token regex");
    let ids: Vec<String> = nodes.keys().cloned().collect();
    for id in ids {
        let (source, name) = nodes
            .get(&id)
            .map(|node| (node.source_code.clone(), node.name.clone()))
            .unwrap_or_default();
        let code = strip_comments_and_strings(&source);
        let code = mask_definition_token(&code, &name, &token_re);
        let mut dependencies = BTreeSet::new();
        for token in token_re.find_iter(&code).map(|value| value.as_str()) {
            if let Some(candidates) = by_name.get(token) {
                for candidate in candidates {
                    if candidate != &id {
                        dependencies.insert(candidate.clone());
                    }
                }
            }
        }
        if let Some(node) = nodes.get_mut(&id) {
            node.depends_on = dependencies.into_iter().collect();
        }
    }
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
    const OOP_TYPES: &[&str] = &["class", "interface", "struct"];

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

fn mask_definition_token(code: &str, qualified_name: &str, token_re: &Regex) -> String {
    let target = qualified_name.rsplit('.').next().unwrap_or(qualified_name);
    let first_line_end = code.find('\n').unwrap_or(code.len());
    let first_line = &code[..first_line_end];
    let mut masked = code.to_string();
    let ranges = token_re
        .find_iter(first_line)
        .filter(|token| token.as_str() == target)
        .map(|token| token.range())
        .collect::<Vec<_>>();
    for range in ranges.into_iter().rev() {
        let length = range.len();
        masked.replace_range(range, &" ".repeat(length));
    }
    masked
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

fn file_component_name(relative: &str) -> String {
    relative
        .rsplit('/')
        .next()
        .unwrap_or(relative)
        .split('.')
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or("File")
        .to_string()
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

    #[test]
    fn extracts_components_for_python() {
        let nodes = extract_components(
            "class Service:\n    def run(self):\n        return 1\n",
            "service.py",
            Path::new("service.py"),
            "Python",
            None,
        );
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].name, "Service");
        assert_eq!(nodes[1].name, "Service.run");
        assert_eq!(nodes[1].id, "service.py::Service.run");
    }

    #[test]
    fn artifact_classifies_manifests() {
        assert_eq!(artifact_class("package.json"), Some("manifest".to_string()));
        assert_eq!(
            artifact_class(".github/workflows/test.yml"),
            Some("ci".to_string())
        );
    }
}
