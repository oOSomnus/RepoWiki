//! Contract for page identity: the one place that knows how a DokuWiki
//! namespace and page ID turn into files on disk.

use repowiki::docs::{
    enumerate_pages, overview_page_id, page_file_path, pages_root, require_canonical_page,
    validate_page_id, PageScope, WikiId,
};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

const CHANGE_ID: &str = "0123456789abcdef0123456789abcdef01234567\
89abcdef0123456789abcdef";

fn write_file(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("file parent")).expect("create parent directory");
    fs::write(path, content).expect("write file");
}

fn write_page(output: &Path, relative: &str, content: &str) {
    write_file(&pages_root(output), relative, content);
}

/// The page tree of an edition, rooted at its output directory.
fn pages(output: &Path) -> PathBuf {
    pages_root(output)
}

/// What the staleness scan takes from an enumeration: the pages this edition
/// owns, as the scan sees them.
fn scanned_ids(output: &Path, wiki: &WikiId) -> Vec<String> {
    enumerate_pages(output, wiki, PageScope::Edition)
        .expect("enumerate edition")
        .into_iter()
        .filter(|page| page.canonical)
        .map(|page| page.page_id)
        .collect()
}

#[test]
fn a_page_id_maps_to_one_directory_per_colon_segment() {
    let temp = tempdir().expect("output tempdir");
    let output = temp.path();
    let wiki = WikiId::parse("repo").expect("parse namespace");

    let overview = page_file_path(output, "repo", "repo:start").expect("map overview");
    assert_eq!(
        overview,
        pages(output).join("repo").join("start.txt"),
        "a single-segment namespace is one directory"
    );
    assert_eq!(
        wiki.dir_under(&pages(output)),
        pages(output).join("repo"),
        "the namespace directory is the same rule"
    );

    assert_eq!(
        page_file_path(output, "repo", "repo:platform:api:start").expect("map module"),
        pages(output)
            .join("repo")
            .join("platform")
            .join("api")
            .join("start.txt"),
        "each colon becomes one level"
    );
    assert_eq!(
        page_file_path(output, CHANGE_ID, &format!("{CHANGE_ID}:start")).expect("map change page"),
        pages(output).join(CHANGE_ID).join("start.txt")
    );
    assert_eq!(
        page_file_path(
            output,
            "repo",
            &overview_page_id("repo").expect("overview ID")
        )
        .expect("map by overview ID"),
        overview,
        "the overview ID round-trips"
    );
}

#[test]
fn storage_and_page_directories_follow_the_same_namespace_rule() {
    // DokuWiki turns `:` into a path separator for pages, attics and metadata
    // alike, so a namespace with more than one segment has to nest everywhere.
    // Reading `attic`/`meta` as one directory named after the whole ID would
    // silently find nothing to merge.
    let temp = tempdir().expect("output tempdir");
    let nested = WikiId::parse("repo:archive").expect("parse nested namespace");
    assert_eq!(
        nested.dir_under(&pages(temp.path())),
        pages(temp.path()).join("repo").join("archive")
    );
    assert_eq!(
        nested.dir_under(&temp.path().join("dokuwiki").join("data").join("meta")),
        temp.path()
            .join("dokuwiki")
            .join("data")
            .join("meta")
            .join("repo")
            .join("archive"),
        "metadata nests exactly like pages"
    );
}

#[test]
fn ids_outside_the_namespace_are_refused_before_any_path_is_built() {
    let temp = tempdir().expect("output tempdir");
    assert!(validate_page_id("repo", "repo:start").is_ok());
    for foreign in [
        "other:start",
        "start",
        "repo:Start",
        "repo:a__b",
        "repo:/escape",
    ] {
        assert!(
            validate_page_id("repo", foreign).is_err(),
            "expected refusal for {foreign}"
        );
        assert!(
            page_file_path(temp.path(), "repo", foreign).is_err(),
            "no path should be built for {foreign}"
        );
    }
    assert!(WikiId::parse("").is_err());
    assert!(WikiId::parse("Repo").is_err());
}

#[test]
fn enumeration_reports_every_page_file_the_edition_holds() {
    let temp = tempdir().expect("output tempdir");
    let output = temp.path();
    let wiki = WikiId::parse("repo").expect("parse namespace");
    write_page(output, "repo/start.txt", "====== Overview ======\n");
    write_page(
        output,
        "repo/platform/start.txt",
        "====== Platform ======\n",
    );
    write_page(output, "repo/platform/api/start.txt", "====== API ======\n");
    // Noise the scan must never treat as this edition's pages:
    write_page(output, "other/start.txt", "another namespace\n");
    write_page(output, "repo/notes__odd.txt", "double underscore\n");
    write_page(output, "repo/media/info.md", "not a page\n");
    fs::create_dir_all(pages(output)).expect("create pages root");
    fs::write(pages(output).join("repo__flattened.txt"), "flat\n").expect("write flat page");

    assert_eq!(
        scanned_ids(output, &wiki),
        vec![
            "repo:platform:api:start".to_string(),
            "repo:platform:start".to_string(),
            "repo:start".to_string(),
        ],
        "the edition scope yields exactly what the scan used to find"
    );

    let all = enumerate_pages(output, &wiki, PageScope::All).expect("enumerate all");
    let reported = all
        .iter()
        .map(|page| (page.page_id.as_str(), page.canonical))
        .collect::<Vec<_>>();
    assert_eq!(
        reported,
        vec![
            ("other:start", false),
            ("repo:notes__odd", false),
            ("repo:platform:api:start", true),
            ("repo:platform:start", true),
            ("repo:start", true),
            ("repo__flattened", false),
        ],
        "a quality report sees intruders, in path order; `.md` stays invisible"
    );
    assert!(
        all.iter().all(|page| page.path.is_file()),
        "every reported entry is a file"
    );
}

#[test]
fn an_unusable_tree_is_no_pages_rather_than_a_failure() {
    let temp = tempdir().expect("output tempdir");
    let output = temp.path();
    let wiki = WikiId::parse("repo").expect("parse namespace");
    assert!(enumerate_pages(output, &wiki, PageScope::Edition)
        .expect("enumerate a missing tree")
        .is_empty());
    assert!(enumerate_pages(output, &wiki, PageScope::All)
        .expect("enumerate a missing tree")
        .is_empty());
    write_page(output, "other/start.txt", "a foreign namespace only\n");
    assert!(
        scanned_ids(output, &wiki).is_empty(),
        "an edition with no pages of its own has nothing to update"
    );
}

#[test]
fn a_page_id_reachable_by_two_paths_is_reported_once_per_path() {
    // `pages/repo/a:b.txt` and `pages/repo/a/b.txt` both spell `repo:a:b`.
    // The report has to see both files to name the collision; the scan has to
    // settle on the one DokuWiki would actually serve.
    let temp = tempdir().expect("output tempdir");
    let output = temp.path();
    let wiki = WikiId::parse("repo").expect("parse namespace");
    write_page(output, "repo/a/b.txt", "the stored page\n");
    write_page(output, "repo/a:b.txt", "the collision\n");

    let all = enumerate_pages(output, &wiki, PageScope::All).expect("enumerate");
    let collisions = all
        .iter()
        .filter(|page| page.page_id == "repo:a:b")
        .collect::<Vec<_>>();
    assert_eq!(collisions.len(), 2, "both files are reported");
    assert_eq!(
        collisions.iter().filter(|page| page.canonical).count(),
        1,
        "only the nested spelling is canonical"
    );
    assert_eq!(
        scanned_ids(output, &wiki),
        vec!["repo:a:b".to_string()],
        "the scan sees one page, not two verdicts"
    );
}

#[cfg(unix)]
#[test]
fn a_symlinked_component_is_read_as_nothing_and_refused_as_something() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().expect("output tempdir");
    let output = temp.path();
    let wiki = WikiId::parse("repo").expect("parse namespace");
    write_file(output, "elsewhere/repo/start.txt", "the real page\n");
    fs::create_dir_all(pages(output)).expect("create pages root");
    symlink(
        output.join("elsewhere").join("repo"),
        pages(output).join("repo"),
    )
    .expect("symlink the namespace");

    let error = require_canonical_page(output, &wiki, "repo:start").unwrap_err();
    assert!(error.to_string().contains("contains a symlink"), "{error}");
    assert!(
        enumerate_pages(output, &wiki, PageScope::All)
            .expect("enumerate")
            .is_empty(),
        "a namespace reached through a symlink is not trusted with a page ID"
    );

    fs::remove_file(pages(output).join("repo")).expect("remove symlink");
    let missing = require_canonical_page(output, &wiki, "repo:start").unwrap_err();
    assert!(
        missing.to_string().contains("is missing"),
        "a required page that is not there is refused: {missing}"
    );

    fs::create_dir_all(pages(output).join("repo").join("start.txt"))
        .expect("shadow the page with a directory");
    let directory = require_canonical_page(output, &wiki, "repo:start").unwrap_err();
    assert!(
        directory.to_string().contains("invalid component"),
        "a directory cannot be a page: {directory}"
    );
}

#[test]
fn a_required_page_is_returned_ready_to_read() {
    let temp = tempdir().expect("output tempdir");
    let output = temp.path();
    let wiki = WikiId::parse("repo").expect("parse namespace");
    write_page(
        output,
        "repo/platform/start.txt",
        "====== Platform ======\n",
    );

    let path = require_canonical_page(output, &wiki, "repo:platform:start").expect("require page");
    assert_eq!(
        path.extension().and_then(|value| value.to_str()),
        Some("txt")
    );
    assert_eq!(
        fs::read_to_string(&path).expect("read page"),
        "====== Platform ======\n"
    );
    assert!(
        require_canonical_page(output, &wiki, "repo:platform:missing").is_err(),
        "a page nobody wrote is not silently mapped"
    );
}

#[test]
fn only_txt_files_are_pages() {
    let temp = tempdir().expect("output tempdir");
    let output = temp.path();
    let wiki = WikiId::parse("repo").expect("parse namespace");
    write_page(output, "repo/start.txt", "====== Overview ======\n");
    write_page(output, "repo/start.txt.bak", "draft\n");
    write_page(output, "repo/notes.md", "not a page\n");

    let ids = enumerate_pages(output, &wiki, PageScope::All)
        .expect("enumerate")
        .into_iter()
        .map(|page| page.page_id)
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["repo:start".to_string()]);
}
