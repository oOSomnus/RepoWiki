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
            "C",
            "C#",
            "C++",
            "Java",
            "JavaScript",
            "Kotlin",
            "PHP",
            "Python",
            "Ruby",
            "Scala",
            "TypeScript",
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
