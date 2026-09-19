use crate::docs;
use crate::session::{self, SessionState};
use anyhow::Result;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// Generate a self-contained client-side viewer for the optional HTML output.
///
/// Pages are embedded instead of fetched at runtime so the result works from
/// `file://` as well as from a static host such as GitHub Pages.
pub fn generate(state: &SessionState) -> Result<String> {
    let output = session::output_dir(state);
    let tree: Value = read_json_or_default(
        &output.join("module_tree.json"),
        Value::Object(serde_json::Map::new()),
    );
    let metadata: Value = read_json_or_default(&output.join("metadata.json"), Value::Null);

    let mut pages = BTreeMap::new();
    if output.exists() {
        for entry in fs::read_dir(&output)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("md") {
                continue;
            }
            let stem = path
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_string();
            pages.insert(stem, session::read_text(&path)?);
        }
    }
    let overview = pages.get("overview").cloned().unwrap_or_else(|| {
        "# CodeWiki\n\nThe host agent has not written an overview yet.".to_string()
    });

    let html = format_template(
        script_json(&tree)?,
        script_json(&pages)?,
        script_json(&metadata)?,
    );
    let path = output.join("index.html");
    session::write_text(&path, &html)?;
    let _ = docs::validate_mermaid(&overview);
    Ok(path.to_string_lossy().into_owned())
}

fn read_json_or_default(path: &Path, fallback: Value) -> Value {
    session::read_json(path).unwrap_or(fallback)
}

fn script_json<T: Serialize>(value: &T) -> Result<String> {
    // Prevent user-authored Markdown containing </script> from terminating a
    // data script element. JSON parsing in the browser restores the text.
    Ok(serde_json::to_string(value)?
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026"))
}

fn format_template(tree_json: String, pages_json: String, metadata_json: String) -> String {
    let template = r##"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>CodeWiki</title>
  <style>
    :root { color-scheme: light dark; --bg: #101318; --panel: #191e27; --text: #e8edf5; --muted: #9ba8bb; --accent: #63b3ed; --border: #2a3342; }
    * { box-sizing: border-box; }
    body { margin: 0; background: var(--bg); color: var(--text); font: 15px/1.6 system-ui, sans-serif; }
    header { padding: 18px 4vw 14px; border-bottom: 1px solid var(--border); }
    header strong { font-size: 1.15rem; }
    main { display: grid; grid-template-columns: 300px minmax(0, 1fr); min-height: calc(100vh - 72px); }
    aside { padding: 18px; background: var(--panel); border-right: 1px solid var(--border); overflow: auto; }
    article { padding: 28px 5vw 56px; max-width: 1100px; width: 100%; }
    .muted { color: var(--muted); }
    .tree, .pages { list-style: none; margin: 0; padding: 0; }
    .tree ul { list-style: none; margin: 2px 0 2px 14px; padding-left: 10px; border-left: 1px solid var(--border); }
    .tree button, .pages button { width: 100%; text-align: left; color: var(--text); background: transparent; border: 0; border-radius: 5px; padding: 5px 7px; cursor: pointer; }
    .tree button:hover, .pages button:hover, .selected { background: #26364b !important; color: white !important; }
    .tree .module { color: var(--accent); }
    h1, h2, h3 { line-height: 1.25; }
    a { color: var(--accent); }
    article p { max-width: 88ch; }
    pre { overflow: auto; background: #0b0e12; padding: 16px; border-radius: 8px; }
    code { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; }
    .meta { border-top: 1px solid var(--border); margin-top: 28px; padding-top: 12px; font-size: .85rem; }
    @media (max-width: 760px) { main { grid-template-columns: 1fr; } aside { border-right: 0; border-bottom: 1px solid var(--border); max-height: 42vh; } }
  </style>
</head>
<body>
  <header><strong>CodeWiki</strong> <span class="muted">agent-generated repository documentation</span></header>
  <main>
    <aside>
      <h3>Module tree</h3>
      <ul id="tree" class="tree"></ul>
      <h3>Pages</h3>
      <ul id="pages" class="pages"></ul>
    </aside>
    <article><div id="content"></div><div id="meta" class="meta muted"></div></article>
  </main>
  <script type="application/json" id="codewiki-tree">__CODEWIKI_TREE__</script>
  <script type="application/json" id="codewiki-pages">__CODEWIKI_PAGES__</script>
  <script type="application/json" id="codewiki-metadata">__CODEWIKI_METADATA__</script>
  <script>
    const tree = JSON.parse(document.getElementById('codewiki-tree').textContent);
    const pages = JSON.parse(document.getElementById('codewiki-pages').textContent);
    const metadata = JSON.parse(document.getElementById('codewiki-metadata').textContent);
    const content = document.getElementById('content');
    const pageList = document.getElementById('pages');

    function escapeHtml(value) {
      return String(value).replace(/[&<>"']/g, character => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[character]));
    }

    function markdown(value) {
      const lines = escapeHtml(value).split('\n');
      let output = [], inCode = false;
      for (const line of lines) {
        if (line.startsWith('```')) {
          output.push(inCode ? '</code></pre>' : '<pre><code>');
          inCode = !inCode;
        } else if (inCode) {
          output.push(line);
        } else if (line.startsWith('### ')) {
          output.push('<h3>' + line.slice(4) + '</h3>');
        } else if (line.startsWith('## ')) {
          output.push('<h2>' + line.slice(3) + '</h2>');
        } else if (line.startsWith('# ')) {
          output.push('<h1>' + line.slice(2) + '</h1>');
        } else if (line.trim() === '') {
          output.push('');
        } else {
          output.push('<p>' + line + '</p>');
        }
      }
      return output.join('\n');
    }

    function openPage(name) {
      const page = pages[name] ?? pages[name.replace(/\.md$/, '')];
      if (page === undefined) return;
      content.innerHTML = markdown(page);
      for (const button of pageList.querySelectorAll('button')) button.classList.toggle('selected', button.dataset.page === name);
      history.replaceState(null, '', '#' + encodeURIComponent(name));
    }

    function addTree(parent, modules) {
      for (const [name, module] of Object.entries(modules || {})) {
        const item = document.createElement('li');
        const button = document.createElement('button');
        button.className = 'module';
        button.textContent = name;
        button.addEventListener('click', () => openPage(name));
        item.appendChild(button);
        if (module.children && Object.keys(module.children).length) {
          const children = document.createElement('ul');
          addTree(children, module.children);
          item.appendChild(children);
        }
        parent.appendChild(item);
      }
    }

    for (const name of Object.keys(pages).sort()) {
      const item = document.createElement('li');
      const button = document.createElement('button');
      button.textContent = name + '.md';
      button.dataset.page = name;
      button.addEventListener('click', () => openPage(name));
      item.appendChild(button);
      pageList.appendChild(item);
    }
    addTree(document.getElementById('tree'), tree);
    const metadataText = metadata && metadata.generation_info ?
      'Generated by ' + (metadata.generation_info.main_model || 'host-agent') +
      (metadata.generation_info.timestamp ? ' at ' + metadata.generation_info.timestamp : '') :
      'Generated by host-agent';
    document.getElementById('meta').textContent = metadataText;
    const hashPage = decodeURIComponent(location.hash.slice(1));
    openPage(hashPage && pages[hashPage] !== undefined ? hashPage : (pages.overview !== undefined ? 'overview' : Object.keys(pages)[0]));
  </script>
</body>
</html>
"##;
    template
        .replace("__CODEWIKI_TREE__", &tree_json)
        .replace("__CODEWIKI_PAGES__", &pages_json)
        .replace("__CODEWIKI_METADATA__", &metadata_json)
}

pub fn copy_reference_asset(output: &Path, asset: &Path) -> Result<()> {
    if asset.exists() {
        fs::create_dir_all(output)?;
        fs::copy(asset, output.join(asset.file_name().unwrap_or_default()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::format_template;

    #[test]
    fn viewer_embeds_pages_and_navigation() {
        let html = format_template(
            "{}".to_string(),
            r##"{"overview":"# Overview\n\nhello"}"##.to_string(),
            "null".to_string(),
        );
        assert!(html.contains("codewiki-pages"));
        assert!(html.contains("openPage"));
        assert!(html.contains("# Overview"));
    }
}
