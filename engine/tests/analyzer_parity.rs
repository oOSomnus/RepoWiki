use codewiki::analyzer::{analyze, AnalyzeOptions};
use codewiki::model::ArtifactIndex;
use codewiki::session;
use std::fs;
use tempfile::tempdir;

fn options() -> AnalyzeOptions {
    AnalyzeOptions {
        gitignore: false,
        ..AnalyzeOptions::default()
    }
}

#[test]
fn include_and_exclude_use_fnmatch_path_semantics() {
    let repo = tempdir().expect("repo tempdir");
    let output = tempdir().expect("output tempdir");
    fs::create_dir_all(repo.path().join("src")).expect("src directory");
    fs::create_dir_all(repo.path().join("tests/nested")).expect("tests directory");
    fs::write(repo.path().join("src/app.py"), "def app():\n    return 1\n").expect("app fixture");
    fs::write(
        repo.path().join("tests/nested/test_app.py"),
        "def test_app():\n    return 1\n",
    )
    .expect("test fixture");
    fs::write(
        repo.path().join("src/app.js"),
        "function app() { return 1; }\n",
    )
    .expect("javascript fixture");

    let mut analyze_options = options();
    analyze_options.include = vec!["*.py".to_string()];
    analyze_options.exclude = vec!["tests/**".to_string()];

    let (_, _, nodes) = analyze(repo.path(), output.path(), &analyze_options, None)
        .expect("analyze filtered repository");

    let ids = nodes.keys().cloned().collect::<Vec<_>>();
    assert_eq!(ids, vec!["src/app.py::app"]);
}

#[test]
fn repowiki_bundles_are_not_analyzed_as_source() {
    let repo = tempdir().expect("repo tempdir");
    let output = tempdir().expect("output tempdir");
    fs::create_dir_all(repo.path().join("src")).expect("src directory");
    fs::write(repo.path().join("src/app.py"), "def app():\n    return 1\n").expect("app fixture");

    let range = format!("{}..{}", "a".repeat(40), "b".repeat(40));
    let change = repo.path().join(".repowiki/changes").join(range);
    fs::create_dir_all(&change).expect("change bundle directory");
    fs::write(
        repo.path().join(".repowiki/generated.rs"),
        "fn repository_page_source() {}\n",
    )
    .expect("repository bundle source fixture");
    fs::write(change.join("module.rs"), "fn change_page_source() {}\n")
        .expect("change bundle source fixture");

    let (_, result, nodes) =
        analyze(repo.path(), output.path(), &options(), None).expect("analyze repository");

    assert_eq!(result.summary.supported_files, 1);
    assert_eq!(
        nodes.keys().cloned().collect::<Vec<_>>(),
        vec!["src/app.py::app"]
    );
}

#[test]
fn class_methods_are_qualified_and_dependency_scan_ignores_literals_and_comments() {
    let repo = tempdir().expect("repo tempdir");
    let output = tempdir().expect("output tempdir");
    fs::create_dir_all(repo.path().join("src")).expect("src directory");
    fs::write(
        repo.path().join("src/service.py"),
        "class Service:\n    def run(self):\n        # fake()\n        text = \"fake()\"\n        \"\"\"fake()\"\"\"\n        return helper()\n\ndef helper():\n    return 1\n\nclass Other:\n    def run(self):\n        return 1\n",
    )
    .expect("service fixture");

    let (_, result, nodes) =
        analyze(repo.path(), output.path(), &options(), None).expect("analyze class repository");

    let ids = nodes.keys().cloned().collect::<Vec<_>>();
    assert_eq!(
        ids,
        vec![
            "src/service.py::Other",
            "src/service.py::Other.run",
            "src/service.py::Service",
            "src/service.py::Service.run",
            "src/service.py::helper",
        ]
    );
    let method = nodes
        .get("src/service.py::Service.run")
        .expect("qualified method");
    assert_eq!(method.name, "Service.run");
    assert_eq!(method.class_name.as_deref(), Some("Service"));
    assert_eq!(
        method.depends_on,
        vec!["src/service.py::helper".to_string()]
    );
    assert!(nodes
        .get("src/service.py::Other.run")
        .expect("second qualified method")
        .depends_on
        .is_empty());
    let leaf_nodes: Vec<String> = session::read_json(std::path::Path::new(&result.leaf_nodes_path))
        .expect("read selected leaf nodes");
    assert_eq!(
        leaf_nodes,
        vec!["src/service.py::Other", "src/service.py::Service"]
    );
}

#[test]
fn all_supported_languages_and_dependency_order_are_stable() {
    let repo = tempdir().expect("repo tempdir");
    let output = tempdir().expect("output tempdir");
    let fixtures = [
        (
            "python.py",
            "def zed():\n    return beta() + alpha()\n\ndef alpha():\n    return 1\n\ndef beta():\n    return 2\n",
        ),
        ("java.java", "class JavaSample { void run() {} }\n"),
        ("javascript.js", "function run() { return 1; }\n"),
        ("typescript.ts", "function run() { return 1; }\n"),
        (
            "go.go",
            "package sample\nfunc zed() int { return beta() + alpha() }\nfunc alpha() int { return 1 }\nfunc beta() int { return 2 }\n",
        ),
        (
            "rust.rs",
            "fn zed() -> i32 { beta() + alpha() }\nfn alpha() -> i32 { 1 }\nfn beta() -> i32 { 2 }\n",
        ),
        ("c.c", "int run() { return 1; }\n"),
        ("cpp.cpp", "int run() { return 1; }\n"),
        ("csharp.cs", "class CSharpSample { void run() {} }\n"),
        ("kotlin.kt", "fun run() {}\n"),
        ("php.php", "<?php function run() {}\n"),
        ("ruby.rb", "def run\nend\n"),
        ("scala.scala", "object ScalaSample { def run() = 1 }\n"),
    ];
    for (file, source) in fixtures {
        fs::write(repo.path().join(file), source).expect("language fixture");
    }

    let (_, result, nodes) =
        analyze(repo.path(), output.path(), &options(), None).expect("analyze language repository");

    assert_eq!(result.summary.supported_files, fixtures.len());
    assert_eq!(
        result.summary.languages,
        vec![
            "c",
            "cpp",
            "csharp",
            "go",
            "java",
            "javascript",
            "kotlin",
            "php",
            "python",
            "ruby",
            "rust",
            "scala",
            "typescript",
        ]
    );
    for node in nodes.values() {
        let mut sorted = node.depends_on.clone();
        sorted.sort();
        assert_eq!(node.depends_on, sorted);
    }
    assert_eq!(
        nodes
            .get("python.py::zed")
            .expect("ordered dependency fixture")
            .depends_on,
        vec![
            "python.py::alpha".to_string(),
            "python.py::beta".to_string(),
        ]
    );
}

#[test]
fn rust_impl_and_go_receiver_components_are_qualified() {
    let repo = tempdir().expect("repo tempdir");
    let output = tempdir().expect("output tempdir");
    fs::create_dir_all(repo.path().join("rust")).expect("rust directory");
    fs::create_dir_all(repo.path().join("go")).expect("go directory");
    fs::write(
        repo.path().join("rust/service.rs"),
        "pub struct RustService;\n\ntrait Runner {\n    fn run(&self) -> i32;\n}\n\nimpl RustService {\n    pub fn new() -> Self { RustService }\n    pub fn execute(&self) -> i32 {\n        helper()\n    }\n}\n\nimpl Runner for RustService {\n    fn run(&self) -> i32 { helper() }\n}\n\nfn helper() -> i32 { 1 }\n",
    )
    .expect("rust fixture");
    fs::write(
        repo.path().join("go/service.go"),
        "package service\n\ntype GoService struct{}\n\ntype Runner interface {\n    Run() error\n}\n\nfunc (s *GoService) Execute() error {\n    return helper()\n}\n\nfunc helper() error { return nil }\n",
    )
    .expect("go fixture");

    let (_, _, nodes) = analyze(repo.path(), output.path(), &options(), None)
        .expect("analyze Rust and Go repository");

    assert!(nodes.contains_key("rust/service.rs::RustService"));
    assert!(nodes.contains_key("rust/service.rs::Runner"));
    assert!(nodes.contains_key("rust/service.rs::Runner.run"));
    assert!(nodes.contains_key("rust/service.rs::RustService.new"));
    assert!(nodes.contains_key("rust/service.rs::RustService.run"));
    let rust_method = nodes
        .get("rust/service.rs::RustService.execute")
        .expect("Rust impl method");
    assert_eq!(rust_method.component_type, "method");
    assert_eq!(rust_method.class_name.as_deref(), Some("RustService"));
    assert!(rust_method
        .depends_on
        .contains(&"rust/service.rs::helper".to_string()));

    assert!(nodes.contains_key("go/service.go::GoService"));
    assert!(nodes.contains_key("go/service.go::Runner"));
    assert!(nodes.contains_key("go/service.go::Runner.Run"));
    let go_method = nodes
        .get("go/service.go::GoService.Execute")
        .expect("Go receiver method");
    assert_eq!(go_method.component_type, "method");
    assert_eq!(go_method.class_name.as_deref(), Some("GoService"));
    assert!(go_method
        .depends_on
        .contains(&"go/service.go::helper".to_string()));
}

#[test]
fn duplicate_symbols_do_not_create_dependency_fanout() {
    let repo = tempdir().expect("repo tempdir");
    let output = tempdir().expect("output tempdir");

    for index in 0..32 {
        let source = if index == 0 {
            "fn helper() -> i32 { 1 }\nfn unique() -> i32 { 2 }\n".to_string()
        } else {
            format!("fn helper() -> i32 {{ {index} }}\n")
        };
        fs::write(repo.path().join(format!("module_{index}.rs")), source)
            .expect("duplicate symbol fixture");
    }
    fs::write(
        repo.path().join("caller.rs"),
        "fn caller() -> i32 { helper() + unique() }\n",
    )
    .expect("caller fixture");

    let (_, _, nodes) = analyze(repo.path(), output.path(), &options(), None)
        .expect("analyze duplicate symbol repository");
    let caller = nodes.get("caller.rs::caller").expect("caller component");

    assert_eq!(caller.depends_on, vec!["module_0.rs::unique".to_string()]);
    assert_eq!(
        nodes
            .values()
            .map(|node| node.depends_on.len())
            .sum::<usize>(),
        1
    );
}

#[test]
fn artifact_budget_and_exclude_are_applied_before_indexing() {
    let repo = tempdir().expect("repo tempdir");
    let output = tempdir().expect("output tempdir");
    fs::write(
        repo.path().join("package.json"),
        r#"{"name":"fixture","scripts":{"build":"echo build"}}"#,
    )
    .expect("artifact fixture");

    let mut analyze_options = options();
    analyze_options.artifacts = true;
    analyze_options.artifact_exclude = vec!["package.json".to_string()];
    let (state, result, _) = analyze(repo.path(), output.path(), &analyze_options, None)
        .expect("analyze artifact fixture");
    let index: ArtifactIndex =
        session::read_json(std::path::Path::new(&result.artifact_index_path))
            .expect("read artifact index");
    assert!(index.files.is_empty());
    session::cleanup(repo.path(), &state.session_id).expect("clean artifact session");
}

#[test]
fn artifact_files_are_first_class_components_with_units() {
    let repo = tempdir().expect("repo tempdir");
    let output = tempdir().expect("output tempdir");
    fs::write(
        repo.path().join("package.json"),
        r#"{"name":"fixture","scripts":{"build":"echo build","test":"echo test"}}"#,
    )
    .expect("artifact fixture");

    let mut analyze_options = options();
    analyze_options.artifacts = true;
    let (state, result, nodes) = analyze(repo.path(), output.path(), &analyze_options, None)
        .expect("analyze artifact components");
    assert!(nodes.contains_key("package.json::package.json"));
    assert!(nodes.contains_key("package.json::build"));
    assert!(nodes.contains_key("package.json::test"));
    let index: ArtifactIndex =
        session::read_json(std::path::Path::new(&result.artifact_index_path))
            .expect("read artifact index");
    assert_eq!(
        index.files["package.json"].units,
        vec!["build".to_string(), "test".to_string()]
    );
    session::cleanup(repo.path(), &state.session_id).expect("clean artifact session");
}

#[test]
fn go_and_rust_manifests_are_indexed_as_artifacts() {
    let repo = tempdir().expect("repo tempdir");
    let output = tempdir().expect("output tempdir");
    fs::write(
        repo.path().join("go.mod"),
        "module example.com/service\n\ngo 1.23\n",
    )
    .expect("Go manifest fixture");
    fs::write(
        repo.path().join("go.sum"),
        "example.com/dependency v1.0.0 h1:fixture\n",
    )
    .expect("Go lock fixture");
    fs::write(
        repo.path().join("Cargo.lock"),
        "# This file is automatically generated by Cargo.\nversion = 3\n",
    )
    .expect("Rust lock fixture");

    let mut analyze_options = options();
    analyze_options.artifacts = true;
    let (state, result, nodes) = analyze(repo.path(), output.path(), &analyze_options, None)
        .expect("analyze language manifests");
    let index: ArtifactIndex =
        session::read_json(std::path::Path::new(&result.artifact_index_path))
            .expect("read artifact index");

    for path in ["go.mod", "go.sum", "Cargo.lock"] {
        assert!(index.files.contains_key(path), "missing artifact {path}");
        assert!(nodes.contains_key(&format!("{path}::{path}")));
    }
    assert_eq!(index.files["go.mod"].class, "manifest");
    assert_eq!(index.files["go.sum"].class, "packaging");
    assert_eq!(index.files["Cargo.lock"].class, "packaging");
    session::cleanup(repo.path(), &state.session_id).expect("clean manifest session");
}
