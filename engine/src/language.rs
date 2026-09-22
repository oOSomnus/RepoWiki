//! Language front ends for the repository dependency graph.
//!
//! The analyzer deliberately has one small interface and a language-specific
//! implementation behind it: source text is parsed once, declarations are
//! converted to [`Node`] values, and references are emitted as
//! [`CallRelationship`] values.  Repository-wide name resolution remains in
//! `analyzer.rs`, where all languages can use the same unique-match policy.

use crate::model::{CallRelationship, Node};
use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;
use tree_sitter::{Language, Node as TsNode, Parser};

#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq)]
pub enum LanguageId {
    Python,
    Java,
    JavaScript,
    TypeScript,
    Go,
    Rust,
    C,
    Cpp,
    CSharp,
    Kotlin,
    Php,
    Ruby,
    Scala,
}

impl LanguageId {
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "python" => Some(Self::Python),
            "java" => Some(Self::Java),
            "javascript" => Some(Self::JavaScript),
            "typescript" => Some(Self::TypeScript),
            "go" => Some(Self::Go),
            "rust" => Some(Self::Rust),
            "c" => Some(Self::C),
            "c++" | "cpp" => Some(Self::Cpp),
            "c#" | "csharp" => Some(Self::CSharp),
            "kotlin" => Some(Self::Kotlin),
            "php" => Some(Self::Php),
            "ruby" => Some(Self::Ruby),
            "scala" => Some(Self::Scala),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Python => "python",
            Self::Java => "java",
            Self::JavaScript => "javascript",
            Self::TypeScript => "typescript",
            Self::Go => "go",
            Self::Rust => "rust",
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::CSharp => "csharp",
            Self::Kotlin => "kotlin",
            Self::Php => "php",
            Self::Ruby => "ruby",
            Self::Scala => "scala",
        }
    }

    fn grammar(self, relative_path: &str, source: &str) -> Language {
        let extension = Path::new(relative_path)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        match self {
            Self::Python => tree_sitter_python::LANGUAGE.into(),
            Self::Java => tree_sitter_java::LANGUAGE.into(),
            Self::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Self::TypeScript => {
                if extension == "tsx" {
                    tree_sitter_typescript::LANGUAGE_TSX.into()
                } else {
                    tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
                }
            }
            Self::Go => tree_sitter_go::LANGUAGE.into(),
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
            Self::C => tree_sitter_c::LANGUAGE.into(),
            Self::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            Self::CSharp => tree_sitter_c_sharp::LANGUAGE.into(),
            Self::Kotlin => tree_sitter_kotlin_ng::LANGUAGE.into(),
            Self::Php => {
                if source.contains("<?php") {
                    tree_sitter_php::LANGUAGE_PHP.into()
                } else {
                    tree_sitter_php::LANGUAGE_PHP_ONLY.into()
                }
            }
            Self::Ruby => tree_sitter_ruby::LANGUAGE.into(),
            Self::Scala => tree_sitter_scala::LANGUAGE.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileAnalysis {
    pub nodes: Vec<Node>,
    pub relationships: Vec<CallRelationship>,
}

/// Parse one source file and return its local graph fragment.
pub fn analyze_file(
    source: &str,
    relative_path: &str,
    path: &Path,
    language: &str,
    artifact: Option<&str>,
) -> Result<FileAnalysis> {
    let language = LanguageId::parse(language)
        .with_context(|| format!("unsupported source language: {language}"))?;
    let mut parser = Parser::new();
    let grammar = language.grammar(relative_path, source);
    parser
        .set_language(&grammar)
        .with_context(|| format!("failed to load {language:?} grammar"))?;
    let tree = parser
        .parse(source, None)
        .context("tree-sitter returned no syntax tree")?;
    let mut collector = Collector::new(source, relative_path, path, language, artifact);
    collector.collect_declarations(tree.root_node());
    collector.collect_relationships(tree.root_node(), None);
    Ok(collector.finish())
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum DeclKind {
    Class,
    Record,
    Interface,
    Struct,
    Enum,
    Trait,
    Object,
    Module,
    Namespace,
    Impl,
    Function,
    Method,
    Type,
    Variable,
    Property,
}

impl DeclKind {
    fn is_container(self) -> bool {
        matches!(
            self,
            Self::Class
                | Self::Record
                | Self::Interface
                | Self::Struct
                | Self::Enum
                | Self::Trait
                | Self::Object
                | Self::Module
                | Self::Namespace
                | Self::Impl
        )
    }

    fn is_callable(self) -> bool {
        matches!(self, Self::Function | Self::Method)
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Class => "class",
            Self::Record => "record",
            Self::Interface => "interface",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Trait => "trait",
            Self::Object => "object",
            Self::Module => "module",
            Self::Namespace => "namespace",
            Self::Impl => "impl",
            Self::Function => "function",
            Self::Method => "method",
            Self::Type => "type",
            Self::Variable => "variable",
            Self::Property => "property",
        }
    }
}

#[derive(Debug, Clone)]
struct Declaration {
    start: usize,
    name: String,
    qualified_name: String,
    id: String,
    kind: DeclKind,
    class_name: Option<String>,
    base_classes: Vec<String>,
}

struct Collector<'a> {
    source: &'a str,
    relative_path: &'a str,
    path: &'a Path,
    language: LanguageId,
    artifact: Option<&'a str>,
    declarations: Vec<Declaration>,
    declaration_by_start: HashMap<usize, usize>,
    nodes: Vec<Node>,
    name_index: HashMap<String, Vec<String>>,
    relationships: BTreeMap<(String, String), CallRelationship>,
    used_ids: HashSet<String>,
}

impl<'a> Collector<'a> {
    fn new(
        source: &'a str,
        relative_path: &'a str,
        path: &'a Path,
        language: LanguageId,
        artifact: Option<&'a str>,
    ) -> Self {
        Self {
            source,
            relative_path,
            path,
            language,
            artifact,
            declarations: Vec::new(),
            declaration_by_start: HashMap::new(),
            nodes: Vec::new(),
            name_index: HashMap::new(),
            relationships: BTreeMap::new(),
            used_ids: HashSet::new(),
        }
    }

    fn finish(self) -> FileAnalysis {
        FileAnalysis {
            nodes: self.nodes,
            relationships: self.relationships.into_values().collect(),
        }
    }

    fn collect_declarations(&mut self, root: TsNode<'_>) {
        self.collect_declarations_in(root, &[], 0);
    }

    fn collect_declarations_in(
        &mut self,
        node: TsNode<'_>,
        containers: &[String],
        function_depth: usize,
    ) {
        let mut next_containers = containers.to_vec();
        let mut next_function_depth = function_depth;

        if let Some((kind, raw_name)) = self.declaration_info(node) {
            let mut name = normalize_symbol(&raw_name);
            if kind == DeclKind::Impl && !name.is_empty() {
                let owner = format!("{}::{}", self.relative_path, name);
                for base in self.base_classes(node, kind) {
                    self.add_relationship(&owner, &base, node.start_position().row + 1);
                }
                next_containers.push(name);
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    self.collect_declarations_in(child, &next_containers, next_function_depth);
                }
                return;
            }
            let mut scope_containers = containers.to_vec();
            if kind == DeclKind::Method && scope_containers.is_empty() {
                match self.language {
                    LanguageId::Go => {
                        if let Some(receiver) = go_receiver_name(node, self.source.as_bytes()) {
                            scope_containers.push(receiver);
                        }
                    }
                    LanguageId::Cpp => {
                        if let Some((owner, method)) = name.rsplit_once('.') {
                            scope_containers.push(owner.to_string());
                            name = method.to_string();
                        }
                    }
                    _ => {}
                }
            }
            let eligible = !name.is_empty() && function_depth == 0 && name != "impl";
            if eligible {
                let logical_name = if scope_containers.is_empty() {
                    name.clone()
                } else {
                    format!("{}.{}", scope_containers.join("."), name)
                };
                let id_base = format!("{}::{}", self.relative_path, logical_name);
                let id = if self.used_ids.insert(id_base.clone()) {
                    id_base
                } else {
                    let candidate = format!("{}#{}", id_base, node.start_position().row + 1);
                    self.used_ids.insert(candidate.clone());
                    candidate
                };
                let qualified_name = self.qualified_name(&logical_name);
                let class_name = if scope_containers.is_empty() {
                    None
                } else {
                    Some(scope_containers.join("."))
                };
                let base_classes = self.base_classes(node, kind);
                let component_type = if kind == DeclKind::Function
                    && !scope_containers.is_empty()
                    && !self.is_module_container(&scope_containers)
                {
                    DeclKind::Method
                } else {
                    kind
                };
                let declaration = Declaration {
                    start: node.start_byte(),
                    name: logical_name.clone(),
                    qualified_name: qualified_name.clone(),
                    id: id.clone(),
                    kind: component_type,
                    class_name: if component_type == DeclKind::Method {
                        class_name.clone()
                    } else {
                        None
                    },
                    base_classes: base_classes.clone(),
                };
                let declaration_index = self.declarations.len();
                self.declaration_by_start
                    .insert(declaration.start, declaration_index);
                self.name_index
                    .entry(logical_name.clone())
                    .or_default()
                    .push(id.clone());
                self.name_index
                    .entry(name.clone())
                    .or_default()
                    .push(id.clone());
                self.name_index
                    .entry(qualified_name)
                    .or_default()
                    .push(id.clone());

                let source_code = self.slice(node.start_byte(), node.end_byte());
                let docstring = self.docstring(node);
                self.nodes.push(Node {
                    id: id.clone(),
                    name: logical_name,
                    component_type: component_type.as_str().to_string(),
                    file_path: self.path.to_string_lossy().into_owned(),
                    relative_path: self.relative_path.to_string(),
                    depends_on: Vec::new(),
                    source_code,
                    start_line: node.start_position().row + 1,
                    end_line: node.end_position().row + 1,
                    has_docstring: docstring.is_some(),
                    docstring,
                    parameters: self.parameters(node),
                    node_type: Some(component_type.as_str().to_string()),
                    base_classes,
                    class_name: declaration.class_name.clone(),
                    display_name: Some(format!("{} {}", component_type.as_str(), declaration.name)),
                    language: self.language.as_str().to_string(),
                    qualified_name: declaration.qualified_name.clone(),
                    artifact_class: self.artifact.map(str::to_string),
                });
                self.declarations.push(declaration);

                if kind.is_container() && kind != DeclKind::Namespace {
                    next_containers.push(name);
                }
                if kind.is_callable() {
                    next_function_depth += 1;
                }
            }
        }

        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.collect_declarations_in(child, &next_containers, next_function_depth);
        }
    }

    fn collect_relationships(&mut self, root: TsNode<'_>, active: Option<usize>) {
        self.collect_relationships_in(root, active);
    }

    fn collect_relationships_in(&mut self, node: TsNode<'_>, active: Option<usize>) {
        let current = self
            .declaration_by_start
            .get(&node.start_byte())
            .copied()
            .or(active);

        if let Some(declaration_index) = self.declaration_by_start.get(&node.start_byte()) {
            let (declaration_id, base_classes) = {
                let declaration = &self.declarations[*declaration_index];
                (declaration.id.clone(), declaration.base_classes.clone())
            };
            for base in base_classes {
                self.add_relationship(&declaration_id, &base, node.start_position().row + 1);
            }
        }

        if let Some(caller_index) = current {
            let caller = self.declarations[caller_index].id.clone();
            let kind = node.kind();
            if is_call_node(kind) {
                if let Some(target) = call_target(node, self.source.as_bytes()) {
                    self.add_relationship(&caller, &target, node.start_position().row + 1);
                }
            } else if is_instantiation_node(kind) {
                if let Some(target) = type_target(node, self.source.as_bytes()) {
                    self.add_relationship(&caller, &target, node.start_position().row + 1);
                }
            } else if is_import_node(kind) {
                if let Some(target) = import_target(node, self.source.as_bytes()) {
                    self.add_relationship(&caller, &target, node.start_position().row + 1);
                }
            } else if is_type_reference(node) {
                if let Some(target) = node_text(node, self.source.as_bytes()) {
                    self.add_relationship(&caller, &target, node.start_position().row + 1);
                }
            }
        }

        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.collect_relationships_in(child, current);
        }
    }

    fn add_relationship(&mut self, caller: &str, raw_target: &str, call_line: usize) {
        let target = normalize_symbol(raw_target);
        let caller_name = caller.rsplit("::").next().unwrap_or(caller);
        let caller_owner = caller_name
            .rsplit_once('.')
            .map(|(owner, _)| owner.rsplit('.').next().unwrap_or(owner));
        if target.is_empty()
            || target == caller_name
            || caller_owner == Some(target.as_str())
            || caller_name.ends_with(&format!(".{target}"))
            || is_builtin(self.language, &target)
        {
            return;
        }
        let resolved = unique_name_match(&self.name_index, &target, caller);
        let callee = resolved.clone().unwrap_or_else(|| target.clone());
        if callee == caller {
            return;
        }
        let key = (caller.to_string(), callee.clone());
        self.relationships.entry(key).or_insert(CallRelationship {
            caller: caller.to_string(),
            callee,
            call_line,
            is_resolved: resolved.is_some(),
        });
    }

    fn declaration_info(&self, node: TsNode<'_>) -> Option<(DeclKind, String)> {
        let kind = node.kind();
        let declaration_kind = match kind {
            "class_definition" | "class_specifier" | "class_definition_statement" => {
                DeclKind::Class
            }
            "record_declaration" => DeclKind::Record,
            "class_declaration" if self.language == LanguageId::Kotlin => {
                let text = node_text(node, self.source.as_bytes())
                    .unwrap_or_default()
                    .trim_start()
                    .to_ascii_lowercase();
                if text.starts_with("interface ") || text.starts_with("fun interface ") {
                    DeclKind::Interface
                } else if text.starts_with("enum class ") {
                    DeclKind::Enum
                } else {
                    DeclKind::Class
                }
            }
            "class_declaration" => DeclKind::Class,
            "class" if self.language == LanguageId::Ruby => DeclKind::Class,
            "interface_declaration" | "interface_definition" => DeclKind::Interface,
            "method_signature" | "abstract_method_signature" => DeclKind::Method,
            "struct_item" | "struct_specifier" | "struct_declaration" => DeclKind::Struct,
            "enum_item" | "enum_specifier" | "enum_declaration" => DeclKind::Enum,
            "trait_item" | "trait_definition" | "trait_declaration" => DeclKind::Trait,
            "object_definition" | "object_declaration" | "companion_object" => DeclKind::Object,
            "mod_item" => DeclKind::Module,
            "module" if self.language == LanguageId::Ruby => DeclKind::Module,
            "namespace_definition" if self.language == LanguageId::Cpp => DeclKind::Namespace,
            "namespace_declaration" | "file_scoped_namespace_declaration"
                if self.language == LanguageId::CSharp =>
            {
                return None
            }
            "impl_item" => DeclKind::Impl,
            "function_definition"
            | "function_declaration"
            | "function_item"
            | "function_definition_statement"
            | "function_signature_item"
            | "method_declaration"
            | "method_definition"
            | "method_elem"
            | "method_spec"
            | "function_expression"
            | "arrow_function"
            | "method"
            | "singleton_method" => {
                if kind == "method_declaration"
                    || kind == "method_definition"
                    || kind == "method_elem"
                    || kind == "method_spec"
                    || (kind == "method" && self.language != LanguageId::Ruby)
                    || (kind == "singleton_method" && self.language != LanguageId::Ruby)
                {
                    DeclKind::Method
                } else {
                    DeclKind::Function
                }
            }
            "field_definition"
                if matches!(
                    self.language,
                    LanguageId::JavaScript | LanguageId::TypeScript
                ) =>
            {
                if has_descendant_kind(node, "arrow_function")
                    || has_descendant_kind(node, "function_expression")
                {
                    DeclKind::Function
                } else {
                    return None;
                }
            }
            "delegate_declaration" | "annotation_type_declaration" => DeclKind::Type,
            "alias_declaration" | "type_definition" | "type_item" | "type_alias_declaration" => {
                DeclKind::Type
            }
            "declaration" if matches!(self.language, LanguageId::C | LanguageId::Cpp) => {
                if has_descendant_kind(node, "function_declarator") {
                    return None;
                }
                DeclKind::Variable
            }
            "type_declaration" if self.language == LanguageId::Go => {
                if has_descendant_kind(node, "interface_type") {
                    DeclKind::Interface
                } else if has_descendant_kind(node, "struct_type") {
                    DeclKind::Struct
                } else {
                    DeclKind::Type
                }
            }
            "type_declaration" => DeclKind::Type,
            "property_declaration" => DeclKind::Property,
            "variable_declarator"
                if matches!(
                    self.language,
                    LanguageId::JavaScript | LanguageId::TypeScript
                ) =>
            {
                if has_descendant_kind(node, "arrow_function")
                    || has_descendant_kind(node, "function_expression")
                {
                    DeclKind::Function
                } else {
                    DeclKind::Variable
                }
            }
            _ => return None,
        };
        let name = declaration_name(node, self.source.as_bytes())?;
        let declaration_kind = if self.language == LanguageId::Cpp
            && declaration_kind == DeclKind::Function
            && name.contains("::")
        {
            DeclKind::Method
        } else {
            declaration_kind
        };
        Some((declaration_kind, name))
    }

    fn base_classes(&self, node: TsNode<'_>, kind: DeclKind) -> Vec<String> {
        let mut result = BTreeSet::new();
        for field in [
            "superclass",
            "super_interfaces",
            "interfaces",
            "base_list",
            "base_class",
            "delegation_specifiers",
            "supertype",
            "extends",
            "implements",
            "trait",
        ] {
            if let Some(child) = node.child_by_field_name(field) {
                collect_type_names(child, self.source.as_bytes(), &mut result);
            }
        }
        if result.is_empty() {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if matches!(
                    child.kind(),
                    "class_heritage"
                        | "base_class_clause"
                        | "base_list"
                        | "delegation_specifiers"
                        | "extends_clause"
                        | "implements_clause"
                ) {
                    collect_type_names(child, self.source.as_bytes(), &mut result);
                }
            }
        }
        if kind == DeclKind::Impl {
            if let Some(child) = node.child_by_field_name("trait") {
                collect_type_names(child, self.source.as_bytes(), &mut result);
            }
        }
        result
            .into_iter()
            .filter(|name| !is_builtin(self.language, name))
            .collect()
    }

    fn parameters(&self, node: TsNode<'_>) -> Vec<String> {
        for field in [
            "parameters",
            "formal_parameters",
            "parameter_list",
            "parameter",
        ] {
            if let Some(parameters) = node.child_by_field_name(field) {
                let mut result = Vec::new();
                let mut cursor = parameters.walk();
                for child in parameters.named_children(&mut cursor) {
                    let text = self.slice(child.start_byte(), child.end_byte());
                    if !text.trim().is_empty() {
                        result.push(text.trim().to_string());
                    }
                }
                if !result.is_empty() {
                    return result;
                }
                let text = self.slice(parameters.start_byte(), parameters.end_byte());
                return split_parameters(&text);
            }
        }
        Vec::new()
    }

    fn docstring(&self, node: TsNode<'_>) -> Option<String> {
        if self.language != LanguageId::Python {
            return None;
        }
        let body = node.child_by_field_name("body")?;
        let first = body.named_child(0)?;
        if first.kind() == "expression_statement" {
            let value = first.named_child(0)?;
            if value.kind() == "string" || value.kind() == "concatenated_string" {
                return Some(self.slice(value.start_byte(), value.end_byte()));
            }
        }
        None
    }

    fn qualified_name(&self, logical_name: &str) -> String {
        let mut module = self.relative_path.replace(['/', '\\'], ".");
        for extension in [
            ".py", ".pyx", ".java", ".js", ".jsx", ".mjs", ".ts", ".tsx", ".go", ".rs", ".c", ".h",
            ".cc", ".cpp", ".cxx", ".c++", ".hpp", ".hxx", ".h++", ".cs", ".kt", ".kts", ".php",
            ".phtml", ".inc", ".rb", ".rake", ".scala", ".sc",
        ] {
            if module.ends_with(extension) {
                module.truncate(module.len() - extension.len());
                break;
            }
        }
        if module.ends_with(".__init__") {
            module.truncate(module.len() - ".__init__".len());
        }
        if module.is_empty() {
            logical_name.to_string()
        } else {
            format!("{module}.{logical_name}")
        }
    }

    fn is_module_container(&self, containers: &[String]) -> bool {
        if containers.is_empty() {
            return false;
        }
        matches!(
            self.declarations
                .iter()
                .rev()
                .find(|declaration| declaration.name.ends_with(&containers.join(".")))
                .map(|declaration| declaration.kind),
            Some(DeclKind::Module | DeclKind::Namespace)
        )
    }

    fn slice(&self, start: usize, end: usize) -> String {
        self.source.get(start..end).unwrap_or_default().to_string()
    }
}

fn declaration_name(node: TsNode<'_>, source: &[u8]) -> Option<String> {
    if node.kind() == "function_definition" {
        if let Some(declarator) = node.child_by_field_name("declarator") {
            if let Some(name) = qualified_declarator_name(declarator, source) {
                return Some(name);
            }
        }
    }
    for field in ["name", "declarator", "left", "object", "type"] {
        if let Some(child) = node.child_by_field_name(field) {
            if let Some(name) = identifier_from_node(child, source) {
                return Some(name);
            }
        }
    }
    identifier_from_node(node, source)
}

fn qualified_declarator_name(node: TsNode<'_>, source: &[u8]) -> Option<String> {
    if matches!(node.kind(), "qualified_identifier" | "scoped_identifier") {
        let value = node_text(node, source)?;
        if value.contains("::") {
            return Some(value);
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(value) = qualified_declarator_name(child, source) {
            return Some(value);
        }
    }
    None
}

fn identifier_from_node(node: TsNode<'_>, source: &[u8]) -> Option<String> {
    let kind = node.kind();
    if is_identifier_kind(kind) {
        return node_text(node, source);
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if is_identifier_kind(child.kind()) {
            return node_text(child, source);
        }
        if let Some(name) = identifier_from_node(child, source) {
            return Some(name);
        }
    }
    None
}

fn go_receiver_name(node: TsNode<'_>, source: &[u8]) -> Option<String> {
    let receiver = node
        .child_by_field_name("receiver")
        .or_else(|| node.named_child(0))?;
    first_type_identifier(receiver, source)
}

fn first_type_identifier(node: TsNode<'_>, source: &[u8]) -> Option<String> {
    if matches!(
        node.kind(),
        "type_identifier" | "qualified_type_identifier" | "package_identifier"
    ) {
        return node_text(node, source);
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(value) = first_type_identifier(child, source) {
            return Some(value);
        }
    }
    None
}

fn is_identifier_kind(kind: &str) -> bool {
    matches!(
        kind,
        "identifier"
            | "type_identifier"
            | "field_identifier"
            | "property_identifier"
            | "namespace_identifier"
            | "constant"
            | "class_name"
            | "method_name"
            | "name"
            | "simple_identifier"
            | "upper_case_identifier"
    )
}

fn node_text(node: TsNode<'_>, source: &[u8]) -> Option<String> {
    node.utf8_text(source).ok().map(str::to_string)
}

fn normalize_symbol(value: &str) -> String {
    let mut value = value.trim().trim_matches(|character| {
        matches!(
            character,
            '"' | '\'' | '`' | '(' | ')' | '{' | '}' | ';' | ','
        )
    });
    if let Some(index) = value.find('(') {
        value = &value[..index];
    }
    if let Some(index) = value.find('<') {
        value = &value[..index];
    }
    value = value.trim();
    while value.starts_with("::") {
        value = &value[2..];
    }
    value.replace("::", ".").replace('\\', ".")
}

fn split_parameters(value: &str) -> Vec<String> {
    let value = value.trim().trim_start_matches('(').trim_end_matches(')');
    let mut result = Vec::new();
    let mut start = 0usize;
    let mut angle = 0usize;
    let mut square = 0usize;
    let mut round = 0usize;
    for (index, character) in value.char_indices() {
        match character {
            '<' => angle += 1,
            '>' => angle = angle.saturating_sub(1),
            '[' => square += 1,
            ']' => square = square.saturating_sub(1),
            '(' => round += 1,
            ')' => round = round.saturating_sub(1),
            ',' if angle == 0 && square == 0 && round == 0 => {
                let item = value[start..index].trim();
                if !item.is_empty() {
                    result.push(item.to_string());
                }
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    let item = value[start..].trim();
    if !item.is_empty() {
        result.push(item.to_string());
    }
    result
}

fn collect_type_names(node: TsNode<'_>, source: &[u8], output: &mut BTreeSet<String>) {
    if is_identifier_kind(node.kind()) {
        if let Some(value) = node_text(node, source) {
            let value = normalize_symbol(&value);
            if !value.is_empty() {
                output.insert(value);
            }
        }
        return;
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_type_names(child, source, output);
    }
}

fn has_descendant_kind(node: TsNode<'_>, target: &str) -> bool {
    if node.kind() == target {
        return true;
    }
    let mut cursor = node.walk();
    let result = node
        .named_children(&mut cursor)
        .any(|child| has_descendant_kind(child, target));
    result
}

fn unique_name_match(
    index: &HashMap<String, Vec<String>>,
    target: &str,
    caller: &str,
) -> Option<String> {
    let mut candidates = Vec::new();
    for key in [Some(target), target.rsplit(['.', ':']).next()]
        .into_iter()
        .flatten()
    {
        if let Some(values) = index.get(key) {
            for value in values {
                if value != caller && !candidates.contains(value) {
                    candidates.push(value.clone());
                }
            }
        }
    }
    (candidates.len() == 1).then(|| candidates.remove(0))
}

fn is_call_node(kind: &str) -> bool {
    matches!(
        kind,
        "call"
            | "call_expression"
            | "function_call_expression"
            | "method_invocation"
            | "invocation_expression"
            | "member_call_expression"
            | "scoped_call_expression"
            | "command_expression"
    )
}

fn call_target(node: TsNode<'_>, source: &[u8]) -> Option<String> {
    for field in ["function", "method", "name", "callable"] {
        if let Some(child) = node.child_by_field_name(field) {
            return node_text(child, source);
        }
    }
    let mut cursor = node.walk();
    let result = node
        .named_children(&mut cursor)
        .next()
        .and_then(|child| node_text(child, source));
    result
}

fn is_instantiation_node(kind: &str) -> bool {
    matches!(
        kind,
        "new_expression"
            | "object_creation_expression"
            | "object_creation"
            | "new_object"
            | "constructor_invocation"
    )
}

fn type_target(node: TsNode<'_>, source: &[u8]) -> Option<String> {
    for field in ["type", "constructor", "class", "name"] {
        if let Some(child) = node.child_by_field_name(field) {
            return node_text(child, source);
        }
    }
    let mut cursor = node.walk();
    let result = node
        .named_children(&mut cursor)
        .find(|child| child.kind().contains("type") || is_identifier_kind(child.kind()))
        .and_then(|child| node_text(child, source));
    result
}

fn is_import_node(kind: &str) -> bool {
    kind.contains("import")
        || kind.contains("include")
        || matches!(
            kind,
            "using_directive" | "namespace_use_declaration" | "use_declaration"
        )
}

fn import_target(node: TsNode<'_>, source: &[u8]) -> Option<String> {
    for field in ["path", "source", "name", "argument", "imported"] {
        if let Some(child) = node.child_by_field_name(field) {
            return node_text(child, source);
        }
    }
    import_symbol_child(node, source).or_else(|| node_text(node, source))
}

fn import_symbol_child(node: TsNode<'_>, source: &[u8]) -> Option<String> {
    if matches!(
        node.kind(),
        "dotted_name"
            | "scoped_identifier"
            | "qualified_name"
            | "namespace_name"
            | "system_lib_path"
            | "string"
            | "string_fragment"
            | "path"
    ) {
        return node_text(node, source);
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(value) = import_symbol_child(child, source) {
            return Some(value);
        }
    }
    None
}

fn is_type_reference(node: TsNode<'_>) -> bool {
    if !matches!(
        node.kind(),
        "type_identifier"
            | "user_type"
            | "named_type"
            | "scoped_type_identifier"
            | "generic_type"
            | "class_type"
    ) {
        return false;
    }
    let Some(parent) = node.parent() else {
        return false;
    };
    let parent_kind = parent.kind();
    parent_kind.contains("parameter")
        || parent_kind.contains("field")
        || parent_kind.contains("return")
        || parent_kind.contains("type")
        || parent_kind.contains("annotation")
        || parent_kind.contains("argument")
        || parent_kind.contains("declaration")
        || parent_kind.contains("definition")
        || parent_kind.contains("item")
}

fn is_builtin(language: LanguageId, value: &str) -> bool {
    let value = value.rsplit(['.', ':']).next().unwrap_or(value);
    let generic = matches!(
        value,
        "if" | "else"
            | "for"
            | "while"
            | "switch"
            | "catch"
            | "return"
            | "throw"
            | "try"
            | "finally"
            | "new"
            | "self"
            | "this"
            | "super"
            | "true"
            | "false"
            | "null"
            | "nil"
            | "None"
            | "Some"
            | "Ok"
            | "Err"
            | "Option"
            | "Result"
            | "String"
            | "Integer"
            | "Boolean"
            | "Object"
            | "Exception"
            | "println"
            | "print"
            | "len"
            | "make"
            | "append"
            | "require"
            | "include"
            | "raise"
            | "to_s"
    );
    if generic {
        return true;
    }
    match language {
        LanguageId::Python => matches!(value, "list" | "dict" | "set" | "tuple" | "int" | "str"),
        LanguageId::Go => matches!(
            value,
            "byte" | "rune" | "error" | "complex64" | "complex128"
        ),
        LanguageId::Rust => matches!(value, "Vec" | "Box" | "Arc" | "Rc" | "Result"),
        LanguageId::C | LanguageId::Cpp => matches!(value, "size_t" | "NULL"),
        LanguageId::Java | LanguageId::CSharp | LanguageId::Kotlin => {
            matches!(
                value,
                "int"
                    | "long"
                    | "float"
                    | "double"
                    | "char"
                    | "void"
                    | "Int"
                    | "Long"
                    | "Float"
                    | "Double"
                    | "Char"
                    | "Boolean"
                    | "Unit"
                    | "Any"
                    | "Nothing"
                    | "String"
            )
        }
        _ => false,
    }
}
