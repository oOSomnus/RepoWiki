use codewiki::reader::{load_manifest, ReaderCatalog};
use codewiki::session::change_wiki_id;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use tempfile::TempDir;

const MODULE_TREE: &str = r#"{
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
}"#;

fn fixture() -> (TempDir, std::path::PathBuf) {
    let repository = tempfile::tempdir().expect("repository tempdir");
    let wiki = repository.path().join(".repowiki");
    write_edition(
        &wiki,
        "repo",
        "repo-name",
        "====== Repo overview ======\n\nRepository edition content only.\n\n<mermaid>\nflowchart LR\n  Reader --> DokuWiki\n</mermaid>\n",
    );
    (repository, wiki)
}

fn write_edition(root: &std::path::Path, wiki_id: &str, title: &str, overview: &str) {
    fs::create_dir_all(root).expect("create edition root");
    fs::write(root.join("module_tree.json"), MODULE_TREE).expect("write module tree");
    fs::write(
        root.join("metadata.json"),
        serde_json::to_vec(&json!({
            "wiki_id": wiki_id,
            "generation_info": {
                "timestamp": "2026-09-20T12:00:00Z",
                "main_model": "fixture-model",
                "repo_path": format!("/tmp/{title}"),
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

    let pages = root.join("dokuwiki/data/pages").join(wiki_id);
    fs::create_dir_all(pages.join("platform/api")).expect("create module page namespace");
    fs::write(pages.join("start.txt"), overview).expect("write overview page");
    fs::write(
        pages.join("platform/start.txt"),
        "====== Platform ======\n\nPlatform page.\n",
    )
    .expect("write platform page");
    fs::write(
        pages.join("platform/api/start.txt"),
        "====== API ======\n\nAPI page.\n",
    )
    .expect("write API page");
}

fn add_change_editions(wiki: &std::path::Path) -> (String, String) {
    let changes = wiki.join("changes");
    fs::create_dir_all(&changes).expect("create changes directory");
    fs::create_dir(changes.join("not-a-change-range")).expect("create unrelated directory");

    let first = format!("{}..{}", "a".repeat(40), "b".repeat(40));
    let second = format!("{}..{}", "c".repeat(64), "d".repeat(64));
    let first_wiki_id = change_wiki_id(&first).expect("derive first namespace");
    let second_wiki_id = change_wiki_id(&second).expect("derive second namespace");
    write_edition(
        &changes.join(&first),
        &first_wiki_id,
        "repo-name",
        "====== First change ======\n\nFirst change edition only.\n",
    );
    write_edition(
        &changes.join(&second),
        &second_wiki_id,
        "second-change-repo",
        "====== Second change ======\n\nSecond change edition only.\n",
    );
    (first, second)
}

fn write_tree(root: &std::path::Path, tree: &Value) {
    fs::write(
        root.join("module_tree.json"),
        serde_json::to_vec(tree).expect("serialize tree"),
    )
    .expect("write module tree");
}

#[test]
fn catalog_uses_native_shape_and_canonical_hierarchical_page_ids() {
    let (_repository, wiki) = fixture();
    let (first, second) = add_change_editions(&wiki);
    let catalog = load_manifest(&wiki).expect("load catalog");

    assert_eq!(catalog.title, "repo-name");
    assert_eq!(catalog.default_edition, "repository");
    assert_eq!(catalog.editions.len(), 3);
    let repository = &catalog.editions[0];
    assert_eq!(repository.id, "repository");
    assert_eq!(repository.wiki_id, "repo");
    assert_eq!(repository.start_id, "repo:start");
    assert_eq!(repository.label, "repo-name");
    assert_eq!(
        serde_json::to_value(&repository.tree).expect("serialize repository tree"),
        json!([{
            "page_id": "repo:platform:start",
            "title": "Platform",
            "children": [{
                "page_id": "repo:platform:api:start",
                "title": "API",
                "children": []
            }]
        }])
    );

    let first_change = &catalog.editions[1];
    let second_change = &catalog.editions[2];
    assert_eq!(first_change.id, first);
    assert_eq!(first_change.label, "aaaaaaaa..bbbbbbbb");
    assert_eq!(
        first_change.wiki_id,
        "change_f1381f5c2010ac6d365b2297624918e61a860d93a39bf6eb3f93e893a0df3eeb"
    );
    assert_eq!(
        first_change.start_id,
        format!("{}:start", first_change.wiki_id)
    );
    assert_eq!(
        first_change.tree[0].page_id,
        format!("{}:platform:start", first_change.wiki_id)
    );
    assert_eq!(second_change.id, second);
    assert_eq!(second_change.label, "cccccccc..dddddddd");
    assert_eq!(
        second_change.wiki_id,
        "change_a9958c6be672c4357399d02c4253c54309da4cf4df387378f4c940c6982b65ac"
    );
    assert_eq!(second_change.title, "second-change-repo");
    assert_eq!(
        second_change.start_id,
        format!("{}:start", second_change.wiki_id)
    );

    let repository_ids = page_ids(repository);
    for change in [first_change, second_change] {
        let change_ids = page_ids(change);
        assert!(repository_ids.is_disjoint(&change_ids));
        assert_eq!(
            change.start_id.strip_suffix(":start"),
            Some(change.wiki_id.as_str())
        );
    }

    let serialized = serde_json::to_value(&catalog).expect("serialize native catalog");
    let root_keys = serialized
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        root_keys,
        BTreeSet::from(["default_edition", "editions", "title"])
    );
    let edition_keys = serialized["editions"][0]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        edition_keys,
        BTreeSet::from(["id", "label", "start_id", "title", "tree", "wiki_id"])
    );
}

fn page_ids(edition: &codewiki::reader::ReaderEdition) -> BTreeSet<String> {
    fn collect(tree: &[codewiki::reader::NavigationNode], ids: &mut BTreeSet<String>) {
        for node in tree {
            ids.insert(node.page_id.clone());
            collect(&node.children, ids);
        }
    }
    let mut ids = BTreeSet::from([edition.start_id.clone()]);
    collect(&edition.tree, &mut ids);
    ids
}

#[test]
fn changes_only_catalog_defaults_to_the_first_sorted_change() {
    let (_repository, wiki) = fixture();
    let (first, second) = add_change_editions(&wiki);
    fs::remove_file(wiki.join("metadata.json")).expect("remove repository metadata");
    fs::remove_file(wiki.join("module_tree.json")).expect("remove repository tree");
    fs::remove_dir_all(wiki.join("dokuwiki")).expect("remove repository pages");

    let catalog = load_manifest(&wiki).expect("load changes-only catalog");
    assert_eq!(catalog.title, "repo-name");
    assert_eq!(catalog.default_edition, first);
    assert_eq!(catalog.editions.len(), 2);
    assert_eq!(catalog.editions[0].id, first);
    assert_eq!(catalog.editions[1].id, second);
}

#[test]
fn reader_rejects_an_empty_catalog_and_incomplete_change_editions() {
    let empty = tempfile::tempdir().expect("empty wiki directory");
    fs::create_dir_all(empty.path().join("changes/not-a-change-range"))
        .expect("create unrelated change directory");
    let error = load_manifest(empty.path()).expect_err("empty catalog must fail");
    assert!(format!("{error:#}").contains("no valid wiki editions"));

    let (_repository, wiki) = fixture();
    let broken = format!("{}..{}", "e".repeat(40), "f".repeat(40));
    fs::create_dir_all(wiki.join("changes").join(&broken)).expect("create incomplete change");
    let error = load_manifest(&wiki).expect_err("incomplete change edition must fail");
    assert!(format!("{error:#}").contains(&format!("invalid change edition '{broken}'")));
}

#[test]
fn reader_requires_metadata_namespace_to_match_each_edition() {
    let (_repository, wiki) = fixture();
    fs::write(
        wiki.join("metadata.json"),
        serde_json::to_vec(
            &json!({"wiki_id":"wrong","generation_info":{"repo_path":"/tmp/repo-name"}}),
        )
        .expect("serialize mismatched metadata"),
    )
    .expect("write mismatched metadata");
    let error = load_manifest(&wiki).expect_err("repository namespace mismatch must fail");
    assert!(format!("{error:#}").contains("metadata wiki_id must be 'repo'"));

    let (_repository, wiki) = fixture();
    let (change, _) = add_change_editions(&wiki);
    let change_root = wiki.join("changes").join(&change);
    fs::write(
        change_root.join("metadata.json"),
        serde_json::to_vec(&json!({"wiki_id":"repo"}))
            .expect("serialize mismatched change metadata"),
    )
    .expect("write mismatched change metadata");
    let error = load_manifest(&wiki).expect_err("change namespace mismatch must fail");
    assert!(format!("{error:#}").contains("metadata wiki_id must be 'change_"));
}

#[test]
fn reader_rejects_missing_required_canonical_txt_pages() {
    for (relative, page_id) in [
        ("repo/start.txt", "repo:start"),
        ("repo/platform/start.txt", "repo:platform:start"),
        ("repo/platform/api/start.txt", "repo:platform:api:start"),
    ] {
        let (_repository, wiki) = fixture();
        fs::remove_file(wiki.join("dokuwiki/data/pages").join(relative))
            .expect("remove required page");
        let error = load_manifest(&wiki).expect_err("missing canonical page must fail");
        let message = format!("{error:#}");
        assert!(message.contains(page_id), "unexpected error: {message}");
        assert!(
            message.contains("required documentation page"),
            "unexpected error: {message}"
        );
    }

    let (_repository, wiki) = fixture();
    let (change, _) = add_change_editions(&wiki);
    let wiki_id = change_wiki_id(&change).expect("derive change namespace");
    fs::remove_file(
        wiki.join("changes")
            .join(&change)
            .join("dokuwiki/data/pages")
            .join(&wiki_id)
            .join("platform/api/start.txt"),
    )
    .expect("remove change module page");
    let error = load_manifest(&wiki).expect_err("missing change page must fail");
    let message = format!("{error:#}");
    assert!(message.contains(&format!("{wiki_id}:platform:api:start")));
}

#[test]
fn reader_rejects_invalid_and_colliding_canonical_module_ids() {
    let (_repository, wiki) = fixture();
    write_tree(
        &wiki,
        &json!({
            "Platform API": {"path":"src/platform","components":[],"children":{}}
        }),
    );
    let error = load_manifest(&wiki).expect_err("invalid module name must fail");
    assert!(format!("{error:#}").contains("invalid module name"));

    let (_repository, wiki) = fixture();
    let (change, _) = add_change_editions(&wiki);
    let change_root = wiki.join("changes").join(&change);
    write_tree(
        &change_root,
        &json!({
            "Invalid name": {"path":"src/invalid","components":[],"children":{}}
        }),
    );
    let error = load_manifest(&wiki).expect_err("invalid change tree must fail");
    let message = format!("{error:#}");
    assert!(message.contains(&format!("invalid change edition '{change}'")));
    assert!(message.contains("invalid module name"));

    let (_repository, wiki) = fixture();
    write_tree(
        &wiki,
        &json!({
            "API": {"path":"src/API","components":[],"children":{}},
            "api": {"path":"src/api","components":[],"children":{}}
        }),
    );
    let error = load_manifest(&wiki).expect_err("canonical ID collision must fail");
    assert!(format!("{error:#}").contains("module page ID collision for 'repo:api:start'"));
}

#[test]
fn identical_module_names_under_different_parents_have_distinct_canonical_ids() {
    let (_repository, wiki) = fixture();
    write_tree(
        &wiki,
        &json!({
            "Platform": {
                "path":"src/platform", "components":[],
                "children":{"API":{"path":"src/platform/api","components":[],"children":{}}}
            },
            "Docs": {
                "path":"docs", "components":[],
                "children":{"API":{"path":"docs/api","components":[],"children":{}}}
            }
        }),
    );
    let pages = wiki.join("dokuwiki/data/pages/repo");
    fs::create_dir_all(pages.join("docs/api")).expect("create Docs API page directory");
    fs::write(pages.join("docs/start.txt"), "====== Docs ======\n").expect("write Docs page");
    fs::write(pages.join("docs/api/start.txt"), "====== Docs API ======\n")
        .expect("write Docs API page");

    let catalog = load_manifest(&wiki).expect("same module names in different parents are valid");
    let platform = catalog.editions[0]
        .tree
        .iter()
        .find(|node| node.title == "Platform")
        .expect("Platform navigation node");
    let docs = catalog.editions[0]
        .tree
        .iter()
        .find(|node| node.title == "Docs")
        .expect("Docs navigation node");
    assert_eq!(platform.children[0].page_id, "repo:platform:api:start");
    assert_eq!(docs.children[0].page_id, "repo:docs:api:start");
}

#[cfg(unix)]
#[test]
fn reader_rejects_change_directory_symlinks_and_escaping_changes_roots() {
    use std::os::unix::fs::symlink;

    let (_repository, wiki) = fixture();
    let outside = tempfile::tempdir().expect("external changes directory");
    symlink(outside.path(), wiki.join("changes")).expect("symlink changes outside root");
    assert!(load_manifest(&wiki).is_err());

    let (_repository, wiki) = fixture();
    let changes = wiki.join("changes");
    fs::create_dir_all(&changes).expect("create changes directory");
    let range = format!("{}..{}", "1".repeat(40), "2".repeat(40));
    let outside_edition = wiki.join("outside-change");
    let namespace = change_wiki_id(&range).expect("derive change namespace");
    write_edition(&outside_edition, &namespace, "repo-name", "outside\n");
    symlink(&outside_edition, changes.join(&range)).expect("symlink edition");
    assert!(load_manifest(&wiki).is_err());
}

#[test]
fn reader_rejects_sha_named_change_files_and_symlinked_page_files() {
    let (_repository, wiki) = fixture();
    let changes = wiki.join("changes");
    fs::create_dir_all(&changes).expect("create changes directory");
    let file = format!("{}..{}", "3".repeat(40), "4".repeat(40));
    fs::write(changes.join(file), "not a bundle").expect("write invalid change file");
    assert!(format!(
        "{:#}",
        load_manifest(&wiki).expect_err("SHA file must fail")
    )
    .contains("change edition must be a directory"));

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let (_repository, wiki) = fixture();
        let outside = tempfile::tempdir().expect("page target tempdir");
        let external_page = outside.path().join("page.txt");
        fs::write(&external_page, "external page").expect("write external page");
        fs::remove_file(wiki.join("dokuwiki/data/pages/repo/start.txt")).expect("remove overview");
        symlink(
            external_page,
            wiki.join("dokuwiki/data/pages/repo/start.txt"),
        )
        .expect("symlink overview page");
        assert!(load_manifest(&wiki).is_err());
    }
}

fn http_request(address: SocketAddr, target: &str) -> (u16, Vec<u8>) {
    http_request_with_method(address, target, "GET", &[])
}

fn http_request_with_method(
    address: SocketAddr,
    target: &str,
    method: &str,
    body: &[u8],
) -> (u16, Vec<u8>) {
    let mut stream =
        TcpStream::connect_timeout(&address, Duration::from_secs(2)).expect("connect to DokuWiki");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("set DokuWiki response timeout");
    let mut request = Vec::new();
    write!(
        &mut request,
        "{method} {target} HTTP/1.0\r\nHost: {address}\r\nAccept-Encoding: identity\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .expect("format DokuWiki request");
    request.extend_from_slice(body);
    stream.write_all(&request).expect("write DokuWiki request");
    let mut response = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                assert!(
                    response.len() + read <= 16 * 1024 * 1024,
                    "HTTP response exceeded bound"
                );
                response.extend_from_slice(&buffer[..read]);
            }
            Err(error) => panic!("read DokuWiki response: {error}"),
        }
    }
    let separator = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap_or_else(|| {
            let preview = String::from_utf8_lossy(&response[..response.len().min(512)]);
            panic!(
                "HTTP response headers for {method} {target}; received {} bytes: {preview:?}",
                response.len()
            )
        });
    let headers = std::str::from_utf8(&response[..separator]).expect("UTF-8 HTTP headers");
    let status = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse().ok())
        .expect("HTTP status");
    (status, response[separator + 4..].to_vec())
}

#[cfg(unix)]
struct RunningReader {
    child: Child,
    stopped: bool,
}

#[cfg(unix)]
impl RunningReader {
    fn stop(&mut self) -> std::process::ExitStatus {
        extern "C" {
            fn kill(pid: i32, signal: i32) -> i32;
        }
        if self
            .child
            .try_wait()
            .expect("inspect reader before shutdown")
            .is_none()
        {
            let pid = i32::try_from(self.child.id()).expect("reader process ID fits i32");
            unsafe {
                kill(pid, 2);
            }
        }
        let status = self.child.wait().expect("wait for reader shutdown");
        self.stopped = true;
        status
    }
}

#[cfg(unix)]
impl Drop for RunningReader {
    fn drop(&mut self) {
        extern "C" {
            fn kill(pid: i32, signal: i32) -> i32;
        }
        if !self.stopped && self.child.try_wait().ok().flatten().is_none() {
            if let Ok(pid) = i32::try_from(self.child.id()) {
                unsafe {
                    kill(pid, 2);
                }
            }
        }
        let _ = self.child.wait();
    }
}

#[cfg(unix)]
#[test]
fn binary_serves_native_dokuwiki_catalog_and_canonical_edition_pages() {
    let (_repository, wiki) = fixture();
    let (first_change, second_change) = add_change_editions(&wiki);
    let first_wiki_id = change_wiki_id(&first_change).expect("derive first namespace");
    let second_wiki_id = change_wiki_id(&second_change).expect("derive second namespace");
    for (edition_root, wiki_id, marker) in [
        (
            wiki.clone(),
            "repo".to_string(),
            "RepoEditionSearchMarker".to_string(),
        ),
        (
            wiki.join("changes").join(&first_change),
            first_wiki_id.clone(),
            "FirstChangeSearchMarker".to_string(),
        ),
        (
            wiki.join("changes").join(&second_change),
            second_wiki_id.clone(),
            "SecondChangeSearchMarker".to_string(),
        ),
    ] {
        fs::write(
            edition_root
                .join("dokuwiki/data/pages")
                .join(wiki_id)
                .join("platform/api/start.txt"),
            format!("====== API ======\n\n{marker}\n"),
        )
        .expect("write edition-scoped search marker");
    }
    let expected_catalog = load_manifest(&wiki).expect("validated catalog");
    let child = Command::new(env!("CARGO_BIN_EXE_repowiki-reader"))
        .arg(&wiki)
        .args(["--port", "0", "--no-open"])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("start actual DokuWiki reader");
    let mut reader = RunningReader {
        child,
        stopped: false,
    };
    let stdout = reader.child.stdout.take().expect("reader stdout");
    let mut lines = BufReader::new(stdout).lines();
    let mut listening = None;
    let mut seen_lines = Vec::new();
    for line in lines.by_ref() {
        let line = line.expect("read reader status");
        seen_lines.push(line.clone());
        if let Some(url) = line.strip_prefix("Listening on ") {
            listening = Some(url.to_string());
            break;
        }
    }
    let url =
        listening.unwrap_or_else(|| panic!("DokuWiki server did not become ready: {seen_lines:?}"));
    let address = url
        .strip_prefix("http://")
        .expect("loopback HTTP URL")
        .parse::<SocketAddr>()
        .expect("reader socket address");

    let (status, catalog_bytes) = http_request(address, "/doku.php?do=repowiki_catalog");
    assert_eq!(status, 200);
    let served_catalog: ReaderCatalog =
        serde_json::from_slice(&catalog_bytes).expect("native catalog JSON response");
    assert_eq!(served_catalog, expected_catalog);
    assert_eq!(
        served_catalog.editions[0].tree[0].children[0].page_id,
        "repo:platform:api:start"
    );

    let (status, repository_html) = http_request(address, "/doku.php?id=repo:start");
    assert_eq!(status, 200);
    let repository_html = String::from_utf8_lossy(&repository_html);
    assert!(
        repository_html.contains("Repository edition content only"),
        "{repository_html}"
    );
    assert!(!repository_html.contains("First change edition only"));
    assert!(
        repository_html.contains("lib/plugins/mermaid/mermaid.min.js"),
        "Mermaid must load from the bundled local asset"
    );
    assert!(
        !repository_html.contains("cdn.jsdelivr.net"),
        "rendered DokuWiki page must not reference a Mermaid CDN"
    );

    let (status, jquery_asset) = http_request(address, "/lib/exe/jquery.php");
    assert_eq!(status, 200);
    assert!(
        String::from_utf8_lossy(&jquery_asset).contains("jQuery v3."),
        "DokuWiki must serve its bundled local jQuery asset"
    );

    assert!(repository_html.contains("data-repowiki-navigation"));
    assert!(repository_html.contains("Search this edition"));
    assert!(
        repository_html.contains("id=\"dokuwiki__aside\""),
        "the navigation tree must live in the template sidebar, not above the page"
    );
    assert!(repository_html.contains("data-repowiki-path"));
    assert!(
        !repository_html.contains("<div class=\"trace\">"),
        "the visit-history trace must be replaced by the ancestor path"
    );

    let (status, api_html) = http_request(address, "/doku.php?id=repo:platform:api:start");
    assert_eq!(status, 200);
    let api_html = String::from_utf8_lossy(&api_html);
    assert!(
        api_html.contains(">Repo overview</a>") && api_html.contains(">Platform</a>"),
        "the ancestor path must use real page titles: {api_html}"
    );
    assert!(
        api_html.contains("class=\"repowiki-path-current\" aria-current=\"page\">API</span>"),
        "the current page title must close the ancestor path: {api_html}"
    );

    let (status, viewer_js) = http_request(address, "/lib/exe/js.php");
    assert_eq!(status, 200);
    assert!(
        String::from_utf8_lossy(&viewer_js).contains("repowiki-viewer"),
        "the diagram viewer must be part of the aggregated DokuWiki script"
    );
    let (status, viewer_css) = http_request(address, "/lib/exe/css.php");
    assert_eq!(status, 200);
    assert!(
        String::from_utf8_lossy(&viewer_css).contains("repowiki-viewer"),
        "the diagram viewer must be part of the aggregated DokuWiki styles"
    );

    let repo_search = "/doku.php?id=repo:start&do=search&q=RepoEditionSearchMarker";
    let (status, repo_search_html) = http_request(address, repo_search);
    assert_eq!(status, 200);
    let repo_search_html = String::from_utf8_lossy(&repo_search_html);
    assert!(repo_search_html.contains("RepoEditionSearchMarker"));
    assert!(
        repo_search_html.contains("data-repowiki-navigation"),
        "actions without a template sidebar must fall back to the inline tree"
    );
    assert!(
        !repo_search_html.contains("data-repowiki-path"),
        "the ancestor path belongs to page views, not search results"
    );
    assert!(
        !repo_search_html.contains(&format!("{first_wiki_id}%3Aplatform%3Aapi%3Astart"))
            && !repo_search_html.contains(&format!("{first_wiki_id}:platform:api:start")),
        "repository search must not include change-edition page results"
    );

    let change_search =
        format!("/doku.php?id={first_wiki_id}:start&do=search&q=FirstChangeSearchMarker");
    let (status, change_search_html) = http_request(address, &change_search);
    assert_eq!(status, 200);
    let change_search_html = String::from_utf8_lossy(&change_search_html);
    assert!(change_search_html.contains("FirstChangeSearchMarker"));

    let cross_edition_search =
        format!("/doku.php?id={first_wiki_id}:start&do=search&q=RepoEditionSearchMarker");
    let (status, cross_edition_search_html) = http_request(address, &cross_edition_search);
    assert_eq!(status, 200);
    let cross_edition_search_html = String::from_utf8_lossy(&cross_edition_search_html);
    assert!(
        !cross_edition_search_html.contains("repo%3Aplatform%3Aapi%3Astart")
            && !cross_edition_search_html.contains("repo:platform:api:start"),
        "change search must not include repository-edition page results"
    );

    let (status, _private_plugin_php) = http_request(address, "/lib/plugins/repowiki/cli.php");
    assert_eq!(status, 404);
    let (status, _raw_page_source) = http_request(address, "/data/pages/repo/start.txt");
    assert_eq!(status, 404);

    let (status, _post_response) = http_request_with_method(
        address,
        "/doku.php?id=repo:start",
        "POST",
        b"do=edit&wikitext=must-not-be-saved",
    );
    assert_eq!(status, 405);

    let first_page = format!("/doku.php?id={first_wiki_id}:start");
    let (status, first_html) = http_request(address, &first_page);
    assert_eq!(status, 200);
    let first_html = String::from_utf8_lossy(&first_html);
    assert!(first_html.contains("First change edition only"));
    assert!(!first_html.contains("Repository edition content only"));

    let second_page = format!("/doku.php?id={second_wiki_id}:start");
    let (status, second_html) = http_request(address, &second_page);
    assert_eq!(status, 200);
    assert!(String::from_utf8_lossy(&second_html).contains("Second change edition only"));

    assert!(
        reader.stop().success(),
        "reader should stop cleanly on Ctrl-C"
    );
}
