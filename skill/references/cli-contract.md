# CLI contract

Normal command results and failures are JSON on stdout. A runtime or argument
failure has this shape and exits non-zero:

~~~json
{"ok": false, "error": "message", "chain": ["message", "cause"]}
~~~

--help and --version are the only intentional text-output exceptions.

The commands use a file-side channel so large code and prompts do not need to travel through stdout. When the current working directory is not the analyzed repository, pass `--repo-root <repo>` to every session-based command: components, prompts, trees, documents, updates, html, and session close/info.

## Session workspace

Sessions live below the analyzed repository:

```text
.codewiki/sessions/<session_id>/
├── state.json
├── component_index.json
├── components.json
├── leaf_nodes.json
├── languages.json
├── summary.json
├── artifact_index.json
├── sources/<safe-id>.src
├── prompts/<prompt-type>-<timestamp>.txt
├── processing_order.json
└── module_tree_validation.json
```

`components read` returns source paths for the requested IDs. The source files begin with component and language comments and are safe to read directly.

## Prompt and update validation

prompt get requires a JSON object whose variables match the selected prompt
contract. Missing required variables, null required values, unknown variable
names, invalid cluster scope, and a non-object vars file are errors. The exact
required and optional variables are in references/prompt-map.md; prompt list
also returns prompt_specs.

generate --update and update plan accept only rung 0, 1, 2, 3, or 3b; the
default is 3. Thresholds are in the inclusive range 0..=1, and
max_diff_tokens must be positive. tau_ren controls token-level rename pairing,
tau_full/tau_tree control full fallback, tau_grow controls reclustering,
tau_nb controls neighbour routing, and k_hop controls upstream context.

## Module tree

The tree input is an object keyed by module name:

```json
{
  "Module_Name": {
    "path": "src/example",
    "components": ["src/example.py::Service"],
    "children": {}
  }
}
```

`tree save --first` writes `first_module_tree.json` and `module_tree.json`, computes leaf-first `processing_order.json`, and validates that every component ID belongs to the analysis. A non-empty `unmatched_component_ids` or `leftover_candidate_ids` means the tree must be repaired before documentation begins.

## Update verdicts

`update finalize` accepts `--verdicts-file <path>`. The file may contain either
a direct object keyed by page name or a reference-style object with a
`verdicts` member. Each value is a verdict string or an object containing
`verdict` and optional `reason`; `.md` suffixes are normalized in the record.

## Document editing

`doc write` creates a new Markdown page and refuses to overwrite it. `doc edit` accepts a JSON array:

```json
[
  {"kind": "str_replace", "old": "old text", "new": "new text"},
  {"kind": "insert", "line": 0, "text": "new first line"}
]
```

`str_replace` requires exactly one match. `insert` is zero-based. `undo` restores the most recent saved version. Mermaid blocks are reported as balanced or unbalanced; validation is best-effort and does not hide the written page.

## Output artifacts

The generated output keeps the reference names: `overview.md`, flat module Markdown files, `module_tree.json`, `first_module_tree.json`, `metadata.json`, `temp/artifact_index.json`, and `temp/dependency_graphs/*_dependency_graph.json`. Incremental runs additionally write `update_record.json`.
