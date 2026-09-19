use codewiki::analyzer::{self, AnalyzeOptions};
use codewiki::session;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

#[test]
fn all_supported_language_extensions_survive_analysis() {
    let repo = tempdir().expect("fixture directory");
    let files = [
        ("service.py", "def run(x):\n    return x\n"),
        ("Service.java", "class Service { void run() {} }\n"),
        ("app.js", "function run(x) { return x; }\n"),
        ("app.ts", "function run(x: number) { return x; }\n"),
        (
            "main.go",
            "package main\nfunc run(x int) int { return x }\n",
        ),
        ("main.rs", "fn run(x: i32) -> i32 { x }\n"),
        ("main.c", "int run(int x) { return x; }\n"),
        ("main.cpp", "int run(int x) { return x; }\n"),
        ("Program.cs", "class Program { void Run() {} }\n"),
        ("Main.kt", "fun run(x: Int): Int = x\n"),
        ("index.php", "<?php function run($x) { return $x; }\n"),
        ("lib.rb", "def run(x)\n  x\nend\n"),
        ("Main.scala", "def run(x: Int): Int = x\n"),
    ];
    for (name, source) in files {
        fs::write(repo.path().join(name), source).expect("fixture source");
    }

    let output = repo.path().join("docs");
    let options = AnalyzeOptions {
        gitignore: false,
        artifacts: false,
        ..Default::default()
    };
    let (state, result, nodes) = analyzer::analyze(repo.path(), &output, &options, None)
        .expect("language matrix should analyze");

    assert_eq!(result.summary.supported_files, files.len());
    assert_eq!(result.summary.languages.len(), files.len());
    assert!(nodes.len() >= files.len());
    for (name, _) in files {
        let path = Path::new(name);
        let expected = match path.extension().and_then(|extension| extension.to_str()) {
            Some("py") => "python",
            Some("java") => "java",
            Some("js") => "javascript",
            Some("ts") => "typescript",
            Some("go") => "go",
            Some("rs") => "rust",
            Some("c") => "c",
            Some("cpp") => "cpp",
            Some("cs") => "csharp",
            Some("kt") => "kotlin",
            Some("php") => "php",
            Some("rb") => "ruby",
            Some("scala") => "scala",
            _ => unreachable!(),
        };
        assert!(
            result
                .summary
                .languages
                .iter()
                .any(|language| language == expected),
            "missing language {expected}"
        );
    }

    session::cleanup(repo.path(), &state.session_id).expect("clean test session");
}
