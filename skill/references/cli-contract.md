# CLI contract

Normal command results and failures are JSON on stdout. A runtime or argument
failure has this shape and exits non-zero:

~~~json
{"ok": false, "error": "message", "chain": ["message", "cause"]}
~~~

--help and --version are the only intentional text-output exceptions.

The commands use a file-side channel so large code and prompts do not need to travel through stdout. When the current working directory is not the analyzed repository, pass `--repo-root <repo>` to every session-based command: components, prompts, trees, documents, updates, html, and session close/info.

For one session, commands that write session or output state are serialized by
the CLI with a cross-process session lock. The host must still issue these
commands serially: `prompt get`, tree save/apply/context, document
write/edit/validate, update writes, HTML generation, and session close. Model
calls may run in parallel only when their CLI writes are not concurrent.
The lock is bounded; a timeout is a JSON error and never silently drops a
write.

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
├── candidate_module_tree.json
├── sources/<safe-id>.src
├── prompts/<prompt-type>-<timestamp>.txt
├── processing_order.json
├── module_tree_validation.json
└── documentation_validation.json
```

`components read` returns source paths for the requested IDs. The source files begin with component and language comments and are safe to read directly.

`candidate_module_tree.json` is an engine-generated structural starting point,
not a final documentation tree. It is built from selected analysis leaves and
may be refined by the host with repeated `scope=module` clustering calls.

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

`update route` writes deterministic suggestions plus `routing_context.json` for
host routing. After the host returns the required `decisions` array,
`update route-apply --decisions-file <path>` places or creates leaves, updates
renamed/deleted IDs, preserves aggregate ownership, and re-runs tree
validation. `update context` writes per-component reports and an orphan context;
it also runs the deterministic stale scan. `update stale-scan` can be run
explicitly, and `update finalize` records its result in `update_record.json`.

## Module tree

The tree input is an object keyed by module name:

```json
{
  "Module_Name": {
    "path": "src/example",
    "components": ["src/example.py::Service"],
    "children": {},
    "decomposition_review": {
      "decision": "retain_leaf",
      "breadth_risk": "low",
      "reason": "The selected components form one cohesive responsibility."
    }
  }
}
```

`decomposition_review.decision` is `split` or `retain_leaf`,
`breadth_risk` is `low`, `medium`, or `high`, and `reason` explains the
source-backed decision. A split module must have children; a retained leaf must
not. Older trees may omit the field for reading and update compatibility.

`tree apply-cluster` consumes a host response file, an input-ID list, and
a working tree. It accepts `<GROUPED_COMPONENTS>` or a JSON object, validates
that selected IDs are real analysis anchors, and merges semantic architecture
groups at repository or module scope. Omitted analysis candidates are
intentional implementation detail and remain only in the dependency graph;
the CLI never creates a fallback page for them. `tree apply-super-group` consumes
`<GROUPED_MODULES>` and nests existing module entries as children without
discarding their pages or IDs. Both commands write the transformed tree to
`--output-tree-file` (or replace `--tree-file`) and return diagnostics. A
module-scope response includes `<DECOMPOSITION_REVIEW>` for the current
parent; it may return an empty `<GROUPED_COMPONENTS>{}</GROUPED_COMPONENTS>`
only when that review says `retain_leaf`.

`tree overview-context` renders a target's structure with components removed,
immediate child `docs_path` values, and a reduced `architecture_context` with
grounded module nodes, dependency edges, and primary paths. A child without an
existing page has `docs_path: null`; an existing page has its resolved path.
Its default target is the repository root and its result is written to the
session workspace.

`tree save --first` writes `first_module_tree.json` and `module_tree.json`, computes leaf-first `processing_order.json` (each item includes its exact canonical `doc_path`), and validates that every selected architecture anchor belongs to the analysis. Module keys must be non-empty ASCII page-safe names. The final tree is intentionally not an exhaustive partition of `leaf_nodes.json`; omitted candidates are recorded as analysis detail rather than treated as missing documentation. On a new generation's final save, pass `--require-decomposition-review` to require a valid review on every module. This flag is optional so older trees and update workflows remain readable.

The final tree may repeat representative component IDs in a parent and its
descendants. `tree save` additionally records these architecture quality fields in
`module_tree_validation.json`:

- `module_count`, `leaf_count`, and `max_depth` describe the saved tree;
- `omitted_analysis_candidate_ids` lists analysis candidates that were not
  selected as architecture anchors;
- `oversized_leaf_modules` reports multi-component leaves whose source-token
  estimate exceeds `max_token_per_module`; `oversized_leaf_warnings` records
  unsplittable singleton leaves. `cluster_batch_size` is a request-size limit
  only and does not make a saved leaf invalid;
- `tree_relationship_errors` reports the same selected anchor being owned by
  multiple leaf modules;
- `decomposition_review` records missing/invalid reviews and warnings for
  high-risk leaves that were retained; warnings remain visible in the
  documentation validation report and do not silently disappear;
- `quality_valid` is false for invalid selected IDs, invalid relationships,
  excessive depth, or oversized architecture pages; `complete` means the
  selected architecture tree is valid.

The engine does not invoke an LLM. The host must perform root clustering,
optional super-grouping, and recursive `scope=module` clustering, then save the
expanded tree. `session close` refuses to remove the session workspace while
the quality gate is false.

The input-ID file must contain either a JSON array of strings or one exact
component ID per line. A JSON object is a prompt-vars file, not an ID list,
and is rejected with an input-format error. Before applying a model response,
the host should run `components read` with the same ID file and confirm that
the non-empty response file already exists.

## Update verdicts

`update finalize` accepts `--verdicts-file <path>`. The file may contain either
a direct object keyed by page name or an object with a `verdicts` member. Each
value is a verdict string or an object containing `verdict` and optional
`reason`; `.md` suffixes are normalized in the record.

## Document editing

`doc write` creates a new Markdown page and refuses to overwrite it. It also
accepts `--if-existing same`: an existing page is treated as success only when
its bytes exactly match the requested content; a different page remains an
error. The result reports whether the page was `created` or `reused`. `doc edit`
accepts a JSON array:

```json
[
  {"kind": "str_replace", "old": "old text", "new": "new text"},
  {"kind": "insert", "line": 0, "text": "new first line"}
]
```

`str_replace` requires exactly one match. `insert` is zero-based. `undo` restores the most recent saved version. Mermaid blocks are reported as balanced or unbalanced; validation is best-effort and does not hide the written page.

## Output artifacts

The generated output contains `overview.md`, semantic module Markdown files,
`module_tree.json`, `first_module_tree.json`, `metadata.json`,
`temp/artifact_index.json`, and `temp/dependency_graphs/*_dependency_graph.json`.
Incremental runs additionally write `update_record.json`.

Metadata statistics distinguish `analysis_leaf_candidates` from generated
`leaf_nodes`; the latter is the number of final module-tree leaves. `module_count`
and `max_depth` describe the documentation tree rather than the analyzer's
candidate selection.

## Documentation quality

After all pages have been written, run:

~~~text
codewiki doc validate --repo-root <repo> --session <session_id>
~~~

The command writes the session-side documentation validation report and
returns per-page roles, language-aware prose counts, source grounding, Mermaid
architecture quality, and required child links. Leaf pages require at least
150 English prose words or 500 CJK prose characters; parent and overview
pages require 200 English prose words or 700 CJK prose characters. Pages also
need two semantic areas and up to two distinct component anchors when
available. A valid report has `valid: true`. It rejects list-only or
fixed-template pages, parent pages with no
useful architecture diagram or child links, and a repository overview without
a grounded end-to-end diagram and top-level links. It also rejects extra
top-level Markdown pages, broken local Markdown links, and non-canonical local
links. Natural CJK prose is counted by Unicode-aware units; do not insert
artificial spaces between Chinese characters. The report is copied into
metadata.json when the session closes. `session close` runs the same check as
a hard gate, so a page file existing on disk is not sufficient.
