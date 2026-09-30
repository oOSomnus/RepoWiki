use crate::docs::{self, overview_page_id};
use crate::dokuwiki;
use crate::model::ModuleTree;
use crate::session::{self, SessionState};
use anyhow::{anyhow, Context, Result};
use regex::{Captures, Regex};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

#[derive(Debug, Clone)]
struct PageRef {
    page_id: String,
    title: String,
}

/// Export native DokuWiki-rendered pages into a single-file navigation shell.
pub fn generate(state: &SessionState) -> Result<String> {
    docs::validate_documentation(state)?;
    let output = session::output_dir(state);
    let tree: ModuleTree = session::read_json(&output.join("module_tree.json"))?;
    let mut expected = BTreeSet::new();
    docs::collect_expected_pages(&state.wiki_id, &tree, &mut expected)?;
    expected.insert(overview_page_id(&state.wiki_id)?);

    let mut pages = vec![PageRef {
        page_id: overview_page_id(&state.wiki_id)?,
        title: "Overview".to_string(),
    }];
    collect_pages(&state.wiki_id, &tree, &[], &mut pages)?;

    let mut navigation = String::new();
    navigation.push_str("<ul><li><a href=\"#");
    navigation.push_str(&page_anchor(&pages[0].page_id));
    navigation.push_str("\">Overview</a></li>");
    render_navigation(&state.wiki_id, &tree, &[], &mut navigation)?;
    navigation.push_str("</ul>");

    let mut sections = String::new();
    let render_context = dokuwiki::session_context(state)?;
    let wiki_id = docs::WikiId::parse(&state.wiki_id)?;
    for page in &pages {
        if !expected.contains(&page.page_id) {
            return Err(anyhow!(
                "HTML export page is absent from canonical page set: {}",
                page.page_id
            ));
        }
        docs::require_canonical_page(&output, &wiki_id, &page.page_id)?;
        let rendered = render_context
            .render(&page.page_id)
            .with_context(|| format!("render DokuWiki page {}", page.page_id))?;
        sections.push_str("<section class=\"wiki-page\" id=\"");
        sections.push_str(&page_anchor(&page.page_id));
        sections.push_str("\" data-page-id=\"");
        sections.push_str(&escape_html(&page.page_id));
        sections.push_str("\" data-page-title=\"");
        sections.push_str(&escape_html(&page.title));
        sections.push_str("\">\n");
        sections.push_str(&rewrite_page_links(&rendered, &expected));
        sections.push_str("\n</section>\n");
    }

    let runtime = &render_context.runtime;
    let assets = ensure_export_assets(&output)?;
    copy_export_asset(
        &runtime.core_dir.join("lib/plugins/mermaid/mermaid.min.js"),
        &assets.join("mermaid.min.js"),
    )
    .context("copy bundled Mermaid renderer")?;
    copy_export_asset(
        &runtime.core_dir.join("lib/plugins/mermaid/mermaid.css"),
        &assets.join("mermaid.css"),
    )
    .context("copy bundled Mermaid styles")?;
    copy_export_asset(
        &runtime.core_dir.join("lib/plugins/mermaid/LICENSE"),
        &assets.join("LICENSE-mermaid-plugin-GPL.txt"),
    )
    .context("copy Mermaid plugin license")?;
    copy_export_asset(
        &runtime.core_dir.join("lib/plugins/mermaid/LICENSE Mermaid"),
        &assets.join("LICENSE-mermaid-js-MIT.txt"),
    )
    .context("copy Mermaid.js license")?;
    copy_export_asset(
        &runtime.integration_dir.join("plugins/repowiki/script.js"),
        &assets.join("repowiki-viewer.js"),
    )
    .context("copy RepoWiki diagram viewer script")?;
    copy_export_asset(
        &runtime.integration_dir.join("plugins/repowiki/style.css"),
        &assets.join("repowiki-viewer.css"),
    )
    .context("copy RepoWiki diagram viewer styles")?;

    let title = Path::new(&state.repo_path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("RepoWiki");
    let html = format!(
        r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{}</title>
<link rel="stylesheet" href="assets/mermaid.css">
<link rel="stylesheet" href="assets/repowiki-viewer.css">
<style>{}</style>
<script defer src="assets/mermaid.min.js"></script>
<script defer src="assets/repowiki-viewer.js"></script>
<script defer>document.addEventListener('DOMContentLoaded',function(){{if(window.mermaid){{mermaid.initialize({{startOnLoad:false,securityLevel:'strict'}});mermaid.run({{querySelector:'.mermaid'}});}}}});</script>
</head>
<body>
<header><h1>{}</h1></header>
<aside aria-label="Wiki navigation">{}</aside>
<main>{}</main>
<footer><small>Rendered with DokuWiki's Mermaid plugin. The plugin is GPLv2; Mermaid.js is MIT. <a href="assets/LICENSE-mermaid-plugin-GPL.txt">Plugin license</a> · <a href="assets/LICENSE-mermaid-js-MIT.txt">Mermaid.js license</a>.</small></footer>
</body>
</html>
"##,
        escape_html(title),
        EXPORT_STYLES,
        escape_html(title),
        navigation,
        sections,
    );
    let path = output.join("index.html");
    session::write_text(&path, &html)?;
    Ok(path.to_string_lossy().into_owned())
}

const EXPORT_STYLES: &str = "body{margin:0;font:16px/1.55 system-ui,sans-serif;color:#17202a;background:#fff}header{padding:1rem 2rem;border-bottom:1px solid #ddd}aside{position:fixed;inset:5.5rem auto 0 0;width:18rem;overflow:auto;padding:1rem 1.5rem;border-right:1px solid #ddd;background:#fafafa}main{margin-left:21rem;padding:1.5rem 3rem;max-width:75rem}.wiki-page{padding:0 0 3rem;margin:0 0 3rem;border-bottom:1px solid #ddd}.wiki-page:target{scroll-margin-top:1rem}aside ul{padding-left:1.25rem}aside li{margin:.3rem 0}pre,code{font-family:ui-monospace,monospace}pre{overflow:auto;padding:.8rem;background:#f5f5f5}.mermaid{display:block;overflow:auto}img{max-width:100%}@media(max-width:850px){aside{position:static;width:auto;border-right:0;border-bottom:1px solid #ddd}main{margin:0;padding:1rem}}";

fn collect_pages(
    wiki_id: &str,
    tree: &ModuleTree,
    parent_path: &[String],
    pages: &mut Vec<PageRef>,
) -> Result<()> {
    for (name, module) in tree {
        let mut path = parent_path.to_vec();
        path.push(name.clone());
        pages.push(PageRef {
            page_id: docs::module_page_id(wiki_id, &path)?,
            title: name.clone(),
        });
        collect_pages(wiki_id, &module.children, &path, pages)?;
    }
    Ok(())
}

fn render_navigation(
    wiki_id: &str,
    tree: &ModuleTree,
    parent_path: &[String],
    html: &mut String,
) -> Result<()> {
    for (name, module) in tree {
        let mut path = parent_path.to_vec();
        path.push(name.clone());
        let page_id = docs::module_page_id(wiki_id, &path)?;
        html.push_str("<li><a href=\"#");
        html.push_str(&page_anchor(&page_id));
        html.push_str("\">");
        html.push_str(&escape_html(name));
        html.push_str("</a>");
        if !module.children.is_empty() {
            html.push_str("<ul>");
            render_navigation(wiki_id, &module.children, &path, html)?;
            html.push_str("</ul>");
        }
        html.push_str("</li>");
    }
    Ok(())
}

fn ensure_export_assets(output: &Path) -> Result<PathBuf> {
    let assets = output.join("assets");
    match fs::symlink_metadata(&assets) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(anyhow!(
                "HTML export asset directory is not a real directory: {}",
                assets.display()
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(&assets)?;
        }
        Err(error) => return Err(error.into()),
    }
    if !assets.canonicalize()?.starts_with(output.canonicalize()?) {
        return Err(anyhow!(
            "HTML export assets escape the wiki output directory: {}",
            assets.display()
        ));
    }
    Ok(assets)
}

fn copy_export_asset(source: &Path, destination: &Path) -> Result<()> {
    match fs::symlink_metadata(destination) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(anyhow!(
                "HTML export asset is not a regular file: {}",
                destination.display()
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    fs::copy(source, destination)?;
    Ok(())
}

fn page_anchor(page_id: &str) -> String {
    format!("page-{}", page_id.replace(':', "--"))
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn rewrite_page_links(html: &str, expected: &BTreeSet<String>) -> String {
    static INTERNAL_LINK: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"href="([^"]*doku\.php\?id=)([^&"]+)(?:&amp;[^"]*)?""#)
            .expect("valid DokuWiki link expression")
    });
    INTERNAL_LINK
        .replace_all(html, |captures: &Captures<'_>| {
            let original = captures.get(0).expect("full match").as_str();
            let prefix = captures.get(1).expect("link prefix").as_str();
            if prefix.contains("://") {
                return original.to_string();
            }
            let encoded_id = captures.get(2).expect("page ID").as_str();
            let decoded_id = percent_decode(encoded_id);
            let page_id = decoded_id.split('#').next().unwrap_or_default();
            if expected.contains(page_id) {
                format!("href=\"#{}\"", page_anchor(page_id))
            } else {
                original.to_string()
            }
        })
        .into_owned()
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2])) {
                decoded.push((high << 4) | low);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(decoded).unwrap_or_else(|_| value.to_string())
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
