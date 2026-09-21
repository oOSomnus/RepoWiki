---
name: repo-wiki
description: Generate architecture-first repository documentation with dependency-aware modules, source-grounded prose, and useful Mermaid diagrams.
metadata:
  short-description: Architecture-first repository wiki generation
---

# Architecture Wiki generation

This Skill generates an architecture reading guide, not a repository index.
The final `module_tree.json` contains only semantic modules that deserve a
reader-facing page. The analyzer may inspect every supported source component,
but low-level functions, tests, generated types, and isolated helpers remain
evidence in the session graph rather than becoming pages.

The host agent owns semantic module selection, few-shot selection, prose, and
Mermaid design. The bundled Rust CLI owns analysis, dependency/context
reduction, prompt transport, safe document writes, and deterministic quality
checks. The CLI never calls an LLM and the host must make the model call for
every clustering and documentation request.

## Runtime

Resolve the directory containing this file and invoke the bundled executable:

- POSIX: `scripts/codewiki`
- Windows: `scripts/codewiki.exe`

Every normal command result is JSON on stdout. A failure has
`{"ok":false,"error":"...","chain":["..."]}` and a non-zero exit code.
Keep large values in session files instead of copying them into chat.

The generation workflow uses these CLI subcommands:

```text
codewiki generate
codewiki components read
codewiki prompt get
codewiki tree save
codewiki tree apply-cluster
codewiki tree apply-super-group
codewiki tree overview-context
codewiki tree order
codewiki doc write
codewiki doc edit
codewiki doc validate
codewiki doc reconcile
codewiki session close
```

Always pass `--repo-root <repo>` to session commands when the analyzed
repository is not the current working directory. Read
[references/cli-contract.md](references/cli-contract.md) and
[references/prompt-map.md](references/prompt-map.md) for command details.

## Host orchestration invariants

The host may parallelize model calls, but it must serialize every CLI command
that writes the session or output tree. In particular, never run concurrent
`doc write`, `tree save`, `tree apply-*`, `tree overview-context`, validation,
update, HTML, or close commands for the same session. The CLI also serializes
these operations defensively; the host still owns the ordering contract.

Keep refinement artifacts in separate files with explicit roles:

```text
<name>.vars.json       prompt variables (JSON object)
<name>.ids.json        input component IDs (JSON string array or one per line)
<name>.response.txt    model clustering response
<name>.content.md      generated Markdown page
```

Before `tree apply-cluster`, confirm that the response file exists and is
non-empty, and run `components read` against the exact input ID file. Never
reuse a prompt-vars file as an ID file. If a page write must be retried, use
`doc write --if-existing same`; it succeeds only when the existing bytes are
identical and never overwrites a different page.

Large prompts, contexts, source listings, and model responses stay in files.
Print only paths, hashes, sizes, and small summaries in the host trace.

## Fresh architecture wiki

1. Start analysis with the architecture-oriented depth:

   ```text
   codewiki generate --repo <repo> --output <repo>/.repowiki --max-depth 2
   ```

   Confirm that the session contains the component graph, source index,
   artifact index, languages, and analysis summary. These are evidence for the
   architecture writer, not pages to expose to readers.

   Before analysis, read the target repository's root `AGENTS.md` when it
   exists, and read nested `AGENTS.md` files that govern source directories
   selected for documentation.

2. Read the analysis summary, dependency graph, selected analysis candidates,
   and relevant source files. Select a small representative set of exact
   component IDs for each semantic subsystem. A selected ID is an evidence
   anchor, not a promise that every component in the file receives a page.

3. Read the relevant examples under `references/few-shots/`. Use:

   - `clickhouse-overview.md` for the repository overview;
   - `clickhouse-storage-engine.md` for parent modules;
   - `clickhouse-query-pipeline.md` for execution/resource flow;
   - `clickhouse-ast-create-query.md` for complex implementation modules.

   Pass one or two selected examples to documentation prompts through the
   `few_shot_examples` variable. They demonstrate information density and
   diagram discipline only. Never copy their facts, headings, or sentences.

4. Request the repository clustering prompt with `scope=repo`. Return only
   semantic architecture modules and a few exact representative component IDs
   per module. Do not force all candidate IDs into groups. Do not create
   pages for tests, generated protocol types, isolated helpers, or directory
   buckets unless they form a genuine system boundary.

   The response must be model-generated. If it is empty, malformed, or
   contains no valid architecture anchors, retry the same request and report
   failure if the retry also fails. Never promote an automatic directory
   bucket or hard-coded Markdown template as a successful model response.

   Keep the prompt variables, exact input IDs, and model response in separate
   files. Do not call `tree apply-cluster` until the response file has been
   written and checked. The CLI's input-ID validation is authoritative, but
   the host should fail earlier with `components read` so malformed IDs are
   diagnosed before tree mutation.

5. Refine only modules that are too broad or contain multiple architectural
   responsibilities. Use `scope=module`, but keep the tree shallow. A child
   must represent a distinct interface, execution stage, state/storage area,
   or integration. Do not split a cohesive subsystem merely because it has
   many functions or files.

6. Save the architecture tree in two phases:

   ```text
   codewiki tree save --repo-root <repo> --session <session_id> --tree-file <root-tree.json> --first
   codewiki tree save --repo-root <repo> --session <session_id> --tree-file <final-tree.json>
   codewiki tree order --repo-root <repo> --session <session_id>
   ```

   The final tree is intentionally lossy with respect to low-level analysis
   components. Its quality gate checks valid evidence IDs, meaningful module
   structure, depth, and page relationships; it does not require exhaustive
   candidate coverage. Use only non-empty ASCII module keys containing letters,
   digits, `_`, or `-`; the processing order exposes the canonical `doc_path`
   that every page writer must use. `first_module_tree.json` is an initial
   snapshot only and must not drive final page generation.

   Treat the returned `tree order` JSON as the only source of page paths and
   write order. Do not reconstruct paths from module names or read the raw
   session file as a substitute for invoking the command.

7. Before each overview prompt, run:

   ```text
   codewiki tree overview-context --repo-root <repo> --session <session_id>
   ```

   The returned context includes the target tree, child page paths, and a
   reduced `architecture_context` containing grounded module nodes, edges, and
   primary paths. Pass the JSON value under `repo_structure` to the prompt's
   `repo_structure` variable and the value under `architecture_context` to its
   `architecture_context` variable; do not pass the wrapper as one opaque
   structure.

8. Generate pages through the model and CLI only:

   - use `system_leaf` and `user` for selected leaf modules;
   - use `system_complex` for selected complex modules;
   - use `overview_module` for parents after child pages exist;
   - use `overview_repo` for `overview.md`.

   The prompts deliberately do not prescribe a heading sequence. The model
   must choose a structure that fits the source. A page must explain purpose,
   interfaces, behavior, and relationships in prose; a component list is not
   documentation. Pass each module's exact `doc_path` to the system prompt and
   write only that path. Keep Chinese prose natural; never add spaces solely
   to inflate the prose count.

   Generate model content in parallel only when useful, then write pages one
   at a time in the `tree order` sequence. Use the canonical `doc_path` from
   each processing item. A retry of an ambiguous page write must use
   `--if-existing same` and must not overwrite a different existing page.

## Mermaid requirements

The repository overview must contain one useful end-to-end architecture
diagram. It should show a concrete path from an external entry point through
real processing/orchestration stages to execution, state, storage, or an
external result. Parent pages should show how their child modules compose.

Use the architecture context to ground node names and edges. Prefer
`flowchart`, `graph`, or `sequenceDiagram` according to the architecture.
Do not create generic diagrams such as `Caller -> Entry -> Core -> Result`,
directory trees, or disconnected module lists. Build/test/release concerns
belong in a separate supporting explanation, not the primary runtime graph.

A leaf page may omit Mermaid when the source has no meaningful interaction to
show. It is better to omit a diagram than to invent one.

## Output and close gate

The published architecture wiki contains:

```text
.repowiki/overview.md
.repowiki/<architecture-module>.md
.repowiki/module_tree.json
.repowiki/first_module_tree.json
.repowiki/metadata.json
```

The session may retain dependency graphs, component source, artifact indexes,
prompt transcripts, and validation reports for generation and diagnosis.

Before close, run:

```text
codewiki doc validate --repo-root <repo> --session <session_id>
codewiki session close --repo-root <repo> --session <session_id>
```

If validation is structurally valid but reports fewer explanatory pages than
total pages, surface that as a quality warning. It does not replace the hard
validity gate, but it should trigger a review of short or list-like pages.

If migrating an older output with Chinese or underscore alias pages, first run
`codewiki doc reconcile` as a dry run, provide an explicit aliases JSON map,
apply it, then run validation. The migration backs up mapped pages and keeps
unmapped extras for manual review.

The report must be valid. It checks that every architecture module has a page,
pages contain source-grounded explanation, parent links are complete, and
overview/parent Mermaid diagrams pass the architecture-quality checks. It does
not require a page for every parsed component.

`codewiki html --repo-root <repo> --session <session_id>` may be run after
validation to publish the static reader view.

## Incremental updates

Use the existing update workflow for architecture pages. Route changed
components to the deepest documented architecture module, not to a low-level
function page. Re-cluster when the change alters a module's responsibility,
public interface, or cross-module flow. A body-only change that does not alter
architecture should update the source anchors or remain unmentioned.

## Completion criteria

The task is complete only when:

- the final tree contains semantic architecture modules and valid source IDs;
- every final tree module has a model-generated page;
- `overview.md` and parent pages link to their documented children;
- overview and parent diagrams pass architecture-quality validation;
- pages are explanatory rather than fixed templates or component inventories;
- `documentation_validation.json` reports `valid: true`;
- metadata records architecture module/anchor counts and documentation quality;
- the host has made the model calls and all writes went through the CLI.

The `reference/CodeWiki` directory is read-only reference material. The
supported input languages are Python, Java, JavaScript, TypeScript, Go, Rust,
C, C++, C#, Kotlin, PHP, Ruby, and Scala.
