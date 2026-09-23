use codewiki::reader::{load_manifest, ReaderEditionKind, WikiReader};
use serde_json::json;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::process::{Command, Stdio};
use tempfile::TempDir;

fn fixture() -> (TempDir, std::path::PathBuf) {
    let repository = tempfile::tempdir().expect("repository tempdir");
    let wiki = repository.path().join(".repowiki");
    fs::create_dir(&wiki).expect("create wiki directory");
    fs::write(
        wiki.join("overview.md"),
        "# Repo overview\n\nA **readable** page with [Platform](Platform.md#architecture).\n\n```rust\nfn main() {}\n```\n\n```mermaid\nflowchart LR\n  Client --> Platform\n```\n",
    )
    .expect("write overview");
    fs::write(
        wiki.join("Platform.md"),
        "# Platform\n\n## Architecture\n\nThe platform boundary.\n",
    )
    .expect("write platform page");
    fs::write(wiki.join("API.md"), "# API\n\nThe public API.\n").expect("write api page");
    fs::write(wiki.join("notes.md"), "# Notes\n\nAn extra page.\n").expect("write extra page");
    fs::write(
        wiki.join("module_tree.json"),
        serde_json::to_vec_pretty(&json!({
            "Platform": {
                "path": "src/platform",
                "components": ["src/platform.rs::Platform"],
                "children": {
                    "API": {
                        "path": "src/api",
                        "components": ["src/api.rs::Api"],
                        "children": {}
                    }
                }
            }
        }))
        .expect("serialize module tree"),
    )
    .expect("write module tree");
    fs::write(
        wiki.join("metadata.json"),
        serde_json::to_vec(&json!({
            "generation_info": {
                "timestamp": "2026-09-20T12:00:00Z",
                "main_model": "fixture-model",
                "repo_path": "/tmp/repo-name",
                "commit_id": "1234567890abcdef"
            },
            "statistics": {
                "total_components": 2,
                "module_count": 2,
                "leaf_nodes": 1
            }
        }))
        .expect("serialize metadata"),
    )
    .expect("write metadata");
    (repository, wiki)
}

fn add_change_editions(wiki: &std::path::Path) -> (String, String) {
    let changes = wiki.join("changes");
    fs::create_dir_all(&changes).expect("create changes directory");
    fs::create_dir(changes.join("not-a-change-range")).expect("create unrelated directory");

    let first = format!("{}..{}", "a".repeat(40), "b".repeat(40));
    let second = format!("{}..{}", "c".repeat(64), "d".repeat(64));
    copy_bundle(
        wiki,
        &changes.join(&first),
        "# First change overview\n\nFirst edition content.\n",
    );
    let second_bundle = changes.join(&second);
    copy_bundle(
        wiki,
        &second_bundle,
        "# Second change overview\n\nSecond edition content.\n",
    );
    fs::write(
        second_bundle.join("metadata.json"),
        serde_json::to_vec(&json!({
            "generation_info": { "repo_path": "/tmp/second-change-repo" }
        }))
        .expect("serialize second change metadata"),
    )
    .expect("write second change metadata");
    (first, second)
}

fn copy_bundle(source: &std::path::Path, destination: &std::path::Path, overview: &str) {
    fs::create_dir_all(destination).expect("create bundle directory");
    for entry in fs::read_dir(source).expect("read source bundle") {
        let entry = entry.expect("read source bundle entry");
        if entry
            .file_type()
            .expect("inspect source bundle entry")
            .is_file()
        {
            fs::copy(entry.path(), destination.join(entry.file_name())).expect("copy bundle file");
        }
    }
    fs::write(destination.join("overview.md"), overview).expect("write bundle overview");
}

#[test]
fn catalog_contains_repository_and_sorted_change_editions() {
    let (_repository, wiki) = fixture();
    let (first, second) = add_change_editions(&wiki);
    let reader = WikiReader::open(&wiki).expect("open catalog reader");
    let catalog = reader.manifest().expect("load catalog");

    assert_eq!(catalog.title, "repo-name");
    assert_eq!(catalog.default_edition, "repository");
    assert_eq!(catalog.editions.len(), 3);
    assert_eq!(catalog.editions[0].id, "repository");
    assert_eq!(catalog.editions[0].kind, ReaderEditionKind::Repository);
    assert_eq!(catalog.editions[0].label, "repo-name");
    assert_eq!(catalog.editions[1].id, first);
    assert_eq!(catalog.editions[1].kind, ReaderEditionKind::Change);
    assert_eq!(catalog.editions[1].label, "aaaaaaaa..bbbbbbbb");
    assert_eq!(catalog.editions[2].id, second);
    assert_eq!(catalog.editions[2].kind, ReaderEditionKind::Change);
    assert_eq!(catalog.editions[2].label, "cccccccc..dddddddd");
    assert_eq!(catalog.editions[2].title, "second-change-repo");

    assert!(reader
        .read_page("repository", "overview.md")
        .unwrap()
        .contains("Repo overview"));
    assert!(reader
        .read_page(&first, "overview.md")
        .unwrap()
        .contains("First edition content"));
    assert!(reader
        .read_page(&second, "overview.md")
        .unwrap()
        .contains("Second edition content"));
    assert!(reader.read_page("unknown", "overview.md").is_err());
}

#[test]
fn changes_only_catalog_defaults_to_first_sorted_change() {
    let (_repository, wiki) = fixture();
    let (first, second) = add_change_editions(&wiki);
    for entry in fs::read_dir(&wiki).expect("read wiki root") {
        let entry = entry.expect("read wiki root entry");
        if entry
            .file_type()
            .expect("inspect wiki root entry")
            .is_file()
        {
            fs::remove_file(entry.path()).expect("remove root bundle file");
        }
    }

    let catalog = load_manifest(&wiki).expect("load changes-only catalog");
    assert_eq!(catalog.title, "repo-name");
    assert_eq!(catalog.default_edition, first);
    assert_eq!(catalog.editions.len(), 2);
    assert_eq!(catalog.editions[0].id, first);
    assert_eq!(catalog.editions[1].id, second);
    assert_eq!(catalog.editions[1].title, "second-change-repo");
}

#[test]
fn reader_requires_at_least_one_valid_edition() {
    let root = tempfile::tempdir().expect("empty wiki directory");
    fs::create_dir_all(root.path().join("changes").join("not-a-change-range"))
        .expect("create unrelated changes directory");

    let error = WikiReader::open(root.path()).expect_err("empty catalog must fail");
    assert!(
        format!("{error:#}").contains("no valid wiki editions"),
        "unexpected error: {error:#}"
    );
}

#[test]
fn sha_named_incomplete_change_bundle_is_rejected() {
    let (_repository, wiki) = fixture();
    let broken = format!("{}..{}", "e".repeat(40), "f".repeat(40));
    fs::create_dir_all(wiki.join("changes").join(&broken))
        .expect("create incomplete change bundle");
    fs::write(
        wiki.join("changes").join(&broken).join("overview.md"),
        "# Incomplete change\n",
    )
    .expect("write incomplete bundle marker");

    let error = WikiReader::open(&wiki).expect_err("incomplete change bundle must fail");
    let message = format!("{error:#}");
    assert!(
        message.contains(&format!("invalid change edition '{broken}'")),
        "unexpected error: {message}"
    );
}

#[cfg(unix)]
#[test]
fn reader_rejects_change_paths_that_escape_the_catalog_root() {
    use std::os::unix::fs::symlink;

    let (_repository, wiki) = fixture();
    let outside = tempfile::tempdir().expect("external changes directory");
    symlink(outside.path(), wiki.join("changes")).expect("symlink changes outside root");
    assert!(
        WikiReader::open(&wiki).is_err(),
        "changes directory outside catalog root must be rejected"
    );
}

#[cfg(unix)]
#[test]
fn reader_rejects_sha_named_change_directory_symlinks() {
    use std::os::unix::fs::symlink;

    let (_repository, wiki) = fixture();
    let changes = wiki.join("changes");
    fs::create_dir_all(&changes).expect("create changes directory");
    let outside = wiki.join("outside-change");
    copy_bundle(&wiki, &outside, "# External change\n");
    let range = format!("{}..{}", "1".repeat(40), "2".repeat(40));
    symlink(&outside, changes.join(range)).expect("symlink change edition");

    assert!(
        WikiReader::open(&wiki).is_err(),
        "symlinked change directory must be rejected"
    );
}

#[test]
fn reader_rejects_sha_named_change_files() {
    let (_repository, wiki) = fixture();
    let changes = wiki.join("changes");
    fs::create_dir_all(&changes).expect("create changes directory");
    let file = format!("{}..{}", "3".repeat(40), "4".repeat(40));
    fs::write(changes.join(file), "not a bundle").expect("write invalid change file");

    let error = WikiReader::open(&wiki).expect_err("SHA-named file must fail");
    assert!(
        format!("{error:#}").contains("change edition must be a directory"),
        "unexpected error: {error:#}"
    );
}

fn http_request(address: SocketAddr, method: &str, target: &str) -> String {
    let mut stream = TcpStream::connect(address).expect("connect reader");
    write!(
        stream,
        "{method} {target} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
    )
    .expect("write HTTP request");
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .expect("read HTTP response");
    String::from_utf8(response).expect("HTTP response is UTF-8")
}

fn response_body(response: &str) -> &str {
    response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .expect("HTTP response headers")
}

#[test]
fn manifest_follows_tree_and_keeps_extra_pages() {
    let (_repository, wiki) = fixture();
    let catalog = load_manifest(&wiki).expect("load reader catalog");
    let repository = &catalog.editions[0];

    assert_eq!(catalog.title, "repo-name");
    assert_eq!(catalog.default_edition, "repository");
    assert_eq!(catalog.editions.len(), 1);
    assert_eq!(repository.id, "repository");
    assert_eq!(repository.kind, ReaderEditionKind::Repository);
    assert_eq!(repository.label, "repo-name");
    assert_eq!(repository.info.model.as_deref(), Some("fixture-model"));
    assert_eq!(repository.info.commit.as_deref(), Some("12345678"));
    assert_eq!(repository.info.total_components, Some(2));
    assert_eq!(repository.navigation[0].name, "Platform");
    assert_eq!(repository.navigation[0].children[0].name, "API");
    assert_eq!(repository.navigation[0].children[0].filename, "API.md");
    assert_eq!(repository.pages[0].filename, "overview.md");
    assert!(repository
        .pages
        .iter()
        .any(|page| page.filename == "notes.md"));
    assert!(repository
        .pages
        .iter()
        .all(|page| page.filename.ends_with(".md")));
}

#[test]
fn reader_rejects_incomplete_current_outputs() {
    for required in [
        "metadata.json",
        "module_tree.json",
        "overview.md",
        "Platform.md",
        "API.md",
    ] {
        let (_repository, wiki) = fixture();
        fs::remove_file(wiki.join(required)).expect("remove required output");
        assert!(
            WikiReader::open(&wiki).is_err(),
            "reader should reject missing {required}"
        );
    }
}

#[test]
fn manifest_rejects_noncanonical_module_page_names() {
    let (_repository, wiki) = fixture();
    fs::write(
        wiki.join("module_tree.json"),
        serde_json::to_vec(&json!({
            "Platform API": {
                "path": "src/platform",
                "components": [],
                "children": {}
            }
        }))
        .expect("serialize invalid module tree"),
    )
    .expect("write invalid module tree");

    let error = load_manifest(&wiki).expect_err("invalid module page name must fail");
    let message = format!("{error:#}");
    assert!(
        message.contains("invalid module name"),
        "unexpected error: {message}"
    );
}

#[test]
fn manifest_rejects_colliding_module_page_names() {
    let (_repository, wiki) = fixture();
    fs::write(
        wiki.join("module_tree.json"),
        serde_json::to_vec(&json!({
            "overview": {
                "path": "src/overview",
                "components": [],
                "children": {
                    "overview_module": {
                        "path": "src/overview_module",
                        "components": [],
                        "children": {}
                    }
                }
            }
        }))
        .expect("serialize colliding module tree"),
    )
    .expect("write colliding module tree");

    let error = load_manifest(&wiki).expect_err("colliding module pages must fail");
    let message = format!("{error:#}");
    assert!(
        message.contains("module page filename collision"),
        "unexpected error: {message}"
    );
}

#[test]
fn page_reads_are_limited_to_direct_markdown_files() {
    let (repository, wiki) = fixture();
    let (change_one, change_two) = add_change_editions(&wiki);
    fs::write(repository.path().join("secret.md"), "not part of the wiki").expect("write secret");
    let reader = WikiReader::open(&wiki).expect("open reader");

    assert!(reader
        .read_page("repository", "overview.md")
        .unwrap()
        .contains("Repo overview"));
    assert!(reader.read_page("repository", "../secret.md").is_err());
    assert!(reader.read_page("repository", "metadata.json").is_err());

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        symlink(repository.path().join("secret.md"), wiki.join("link.md"))
            .expect("create outside symlink");
        symlink(
            wiki.join("changes").join(&change_one).join("overview.md"),
            wiki.join("other-edition.md"),
        )
        .expect("create cross-edition symlink");
        symlink(
            wiki.join("overview.md"),
            wiki.join("changes")
                .join(&change_two)
                .join("other-edition.md"),
        )
        .expect("create reverse cross-edition symlink");
        assert!(reader.read_page("repository", "link.md").is_err());
        assert!(reader.read_page("repository", "other-edition.md").is_err());
        assert!(reader.read_page(&change_two, "other-edition.md").is_err());
    }
}

#[test]
fn binary_serves_offline_assets_manifest_pages_and_safe_errors() {
    let (_repository, wiki) = fixture();
    let (change_one, change_two) = add_change_editions(&wiki);
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        symlink(
            wiki.join("changes").join(&change_one).join("overview.md"),
            wiki.join("cross-edition.md"),
        )
        .expect("create cross-edition page symlink");
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_repowiki-reader"))
        .arg(&wiki)
        .args(["--port", "0", "--no-open"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start reader binary");
    let stdout = child.stdout.take().expect("reader stdout");
    let mut lines = BufReader::new(stdout).lines();
    let mut listening = None;
    let mut status_lines = Vec::new();
    for line in lines.by_ref().take(3) {
        let line = line.expect("read reader status");
        status_lines.push(line.clone());
        if let Some(url) = line.strip_prefix("Listening on ") {
            listening = Some(url.trim_end_matches('/').to_string());
            break;
        }
    }
    let url = match listening {
        Some(url) => url,
        None => {
            let status = child.try_wait().expect("inspect reader status");
            let mut stderr = String::new();
            if status.is_some() {
                child
                    .stderr
                    .take()
                    .expect("reader stderr")
                    .read_to_string(&mut stderr)
                    .expect("read reader stderr");
            }
            let _ = child.kill();
            panic!("reader listening URL missing; lines={status_lines:?}, status={status:?}, stderr={stderr}");
        }
    };
    let address = url
        .strip_prefix("http://")
        .expect("loopback HTTP URL")
        .parse::<SocketAddr>()
        .expect("reader socket address");

    let root = http_request(address, "GET", "/");
    assert!(root.starts_with("HTTP/1.1 200 OK"));
    assert!(root.contains("/assets/vendor/mermaid.min.js"));
    assert!(!root.contains("cdn.jsdelivr.net"));
    for asset in [
        "/assets/app.js",
        "/assets/styles.css",
        "/assets/vendor/marked.min.js",
        "/assets/vendor/marked-gfm-heading-id.min.js",
        "/assets/vendor/mermaid.min.js",
        "/assets/vendor/highlight.min.js",
        "/assets/vendor/highlight-github.min.css",
        "/assets/vendor/highlight-github-dark.min.css",
        "/assets/vendor/purify.min.js",
    ] {
        let response = http_request(address, "GET", asset);
        assert!(
            response.starts_with("HTTP/1.1 200 OK"),
            "asset failed: {asset}"
        );
    }

    let styles = http_request(address, "GET", "/assets/styles.css");
    assert!(response_body(&styles).contains("[hidden] { display: none !important; }"));

    let manifest = http_request(address, "GET", "/api/manifest");
    assert!(manifest.starts_with("HTTP/1.1 200 OK"));
    assert!(response_body(&manifest).contains("\"navigation\""));
    assert!(response_body(&manifest).contains("Platform"));
    assert!(response_body(&manifest).contains("\"default_edition\":\"repository\""));
    assert!(response_body(&manifest).contains("\"kind\":\"change\""));

    let page = http_request(address, "GET", "/api/editions/repository/pages/overview.md");
    assert!(page.starts_with("HTTP/1.1 200 OK"));
    assert!(response_body(&page).contains("Repo overview"));

    #[cfg(unix)]
    {
        let cross_edition = http_request(
            address,
            "GET",
            "/api/editions/repository/pages/cross-edition.md",
        );
        assert!(cross_edition.starts_with("HTTP/1.1 404 Not Found"));
    }

    let first_change = http_request(
        address,
        "GET",
        &format!("/api/editions/{change_one}/pages/overview.md"),
    );
    assert!(first_change.starts_with("HTTP/1.1 200 OK"));
    assert!(response_body(&first_change).contains("First edition content"));

    let second_change = http_request(
        address,
        "GET",
        &format!("/api/editions/{change_two}/pages/overview.md"),
    );
    assert!(second_change.starts_with("HTTP/1.1 200 OK"));
    assert!(response_body(&second_change).contains("Second edition content"));

    let unknown_edition = http_request(address, "GET", "/api/editions/unknown/pages/overview.md");
    assert!(unknown_edition.starts_with("HTTP/1.1 404 Not Found"));

    let removed_route = http_request(address, "GET", "/api/pages/overview.md");
    assert!(removed_route.starts_with("HTTP/1.1 404 Not Found"));

    let traversal = http_request(
        address,
        "GET",
        "/api/editions/repository/pages/%2e%2e%2fsecret.md",
    );
    assert!(traversal.starts_with("HTTP/1.1 404 Not Found"));

    let method = http_request(address, "POST", "/");
    assert!(method.starts_with("HTTP/1.1 405 Method Not Allowed"));
    assert!(method.contains("Allow: GET, HEAD"));

    let head = http_request(
        address,
        "HEAD",
        "/api/editions/repository/pages/overview.md",
    );
    assert!(head.starts_with("HTTP/1.1 200 OK"));
    assert!(response_body(&head).is_empty());

    child.kill().expect("stop reader");
    child.wait().expect("wait for reader");
}
