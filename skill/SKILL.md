---
name: repo-wiki
description: Generate or incrementally update a repository wiki when the agent needs dependency-aware module documentation, overview pages, Mermaid diagrams, and RepoWiki-compatible JSON artifacts using the bundled Rust CLI.
metadata:
  short-description: Agent-driven repository wiki generation
---

# RepoWiki wiki generation

Use this skill when the requested result is a repository-level wiki, architecture documentation, or an incremental wiki update. The host agent owns prose generation; the bundled Rust CLI owns repository analysis, session files, prompt transport, safe document edits, module-tree validation, and bookkeeping.

## Runtime

The user asks for the wiki in natural language. Resolve the directory containing this file, then invoke the bundled executable:

- POSIX: `scripts/codewiki`
- Windows: `scripts/codewiki.exe`

The executable was compiled for the environment that produced this Skill package. Cargo, Rust source, prompt source files, and build scripts are not part of the installed package and are not needed at runtime. Never ask the user to build or invoke the CLI manually. If the executable is missing or cannot run, report that the Skill package is incomplete or built for another platform.

Every normal command result, including runtime and CLI-argument failures, is JSON on stdout. A failure has {"ok":false,"error":"...","chain":["..."]} and a non-zero exit status; --help and --version intentionally print human-readable text. Keep large data in the session files named by command output instead of copying it into chat. The reference-compatible logical tools are CLI subcommands:

```text
analyze_repo          -> codewiki analyze
read_code_components  -> codewiki components read
get_prompt            -> codewiki prompt get
save_module_tree      -> codewiki tree save
get_processing_order  -> codewiki tree order
write_doc_file        -> codewiki doc write
edit_doc_file         -> codewiki doc edit
close_session         -> codewiki session close
```

Read [references/cli-contract.md](references/cli-contract.md) when constructing command input files or interpreting validation JSON. Read [references/prompt-map.md](references/prompt-map.md) when choosing a prompt and its variables. Prompt bodies are embedded in the bundled executable.

Always pass --repo-root <repo> to session-based commands when the agent is not
running with the analyzed repository as its current directory. This includes
components read, prompt get, tree save, tree order, every doc command, all
update commands, html, and session close.

## Fresh wiki

1. Start analysis and capture the returned `session_id`:

   ```text
   codewiki generate --repo <repo> --output <repo>/docs
   ```

   Confirm that the result contains the session workspace, component index, leaf list, language list, dependency graph, and artifact index. Do not start prose generation before these paths exist.

2. Read `summary.json`, `languages.json`, `component_index.json`, and the relevant source files under the session workspace. Request the repository clustering prompt with `codewiki prompt get --repo-root <repo> --session <session_id> --type cluster --vars-file <cluster-vars.json>`. The vars file must be a JSON object containing `potential_core_components`; use `scope=module`, `module_name`, and `module_tree` for module clustering. Have the host agent return the prompt's required grouping structure, preserve component IDs exactly, and write that response as a module-tree JSON input file.

3. Save the clustered tree:

   ```text
   codewiki tree save --repo-root <repo> --session <session_id> --tree-file <tree.json> --first
   codewiki tree order --repo-root <repo> --session <session_id>
   ```

   Continue only when validation reports no unknown component IDs. The processing order must be leaf modules before parents.

4. For each processing-order item, request `system_leaf` or `system_complex`, then `user` with the module name, tree, and component source paths. The host agent writes Markdown content to a temporary file and calls `codewiki doc write --repo-root <repo>` for a new page or `codewiki doc edit --repo-root <repo>` for an existing page. Keep the page name returned by the processing order; do not invent a second filename.

5. After all module pages exist, request `overview_module` for parent pages and `overview_repo` for `overview.md`. The overview must link to child pages using the flat document names and must not inline a child page's full documentation.

6. Confirm the key output contract before closing:

   ```text
   docs/overview.md
   docs/<module>.md
   docs/module_tree.json
   docs/first_module_tree.json
   docs/metadata.json
   docs/update_record.json       # incremental runs
   docs/temp/artifact_index.json
   docs/temp/dependency_graphs/*_dependency_graph.json
   ```

   Optionally run `codewiki html --repo-root <repo> --session <session_id>`. Then close the session with `codewiki session close --repo-root <repo> --session <session_id>`. A successful run has written pages, metadata, and a cleaned session workspace.

## Incremental update

Run `codewiki generate --update` with the same repository and output directory. The default update rung is `3`; accepted values are `0`, `1`, `2`, `3`, and `3b`. Invalid rung or threshold values fail before an update plan is written. Then execute the returned update plan, route, and context commands with `--repo-root <repo>`. Use the generated reports and write sets to decide which existing pages need edits. Route every host-agent edit through the CLI so the write guard, edit history, and Mermaid report remain authoritative.

Use the update prompt types for each active leaf. End every update-agent response with the required fenced JSON verdict, record the verdicts in the update workflow, and run:

```text
codewiki update finalize --repo-root <repo> --session <session_id> --model host-agent
```

Before finalizing, write the host agent's page decisions to a JSON file and
pass `--verdicts-file <path>`. The file may be either a direct page map or the
reference-shaped envelope `{"verdicts": {"page.md": {"verdict": "patch", "reason": "..."}}}`.
The CLI stores the normalized verdicts and generated report names in
`update_record.json`.

Use rung `1` for safe incremental edits, `2` for leaf rewrites, `3` for the full updater, and `3b` for the full updater with wider k-hop context. `tau_ren` controls token-level rename pairing; `tau_full` and `tau_tree` control full-build fallback; `tau_grow` controls reclustering; `tau_nb` controls neighbour-majority routing; `k_hop` controls upstream context; and `max_diff_tokens` caps report source. Treat a `full_fallback` plan as a rebuild request, not as a partial update. The completed update writes `docs/update_record.json`.

## Completion criteria

The task is complete only when the requested pages and JSON artifacts are present, `module_tree_validation.json` has no invalid IDs, the processing order has been consumed, every intended page write was made through the CLI, and metadata/update records describe the run. Generated prose may differ from the reference wording; artifact names, structural fields, dependency IDs, module relationships, and lifecycle semantics are the compatibility target.

## Boundaries

The CLI does not call an LLM, store provider credentials, start an MCP server, or run the reference web application. The supported input languages are Python, Java, JavaScript, TypeScript, Go, Rust, C, C++, C#, Kotlin, PHP, Ruby, and Scala. The `reference/CodeWiki` directory is read-only reference material and is excluded from the Skill package.
