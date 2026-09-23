# Change Wiki workflow

This reference owns the range, path, and cleanup rules for `/change-wiki`. The
normal RepoWiki command and page contracts are in `cli-contract.md` and
`prompt-map.md`; reuse them rather than inventing a second CLI workflow.

## Range preflight

Resolve the original Git worktree root, canonicalize it, and retain its
basename. Parse the invocation as exactly one non-empty `base-ref...head-ref`.
Treat both ref strings as untrusted argv values. Use a process API with an argv
array and no shell interpolation; for example:

```text
git -C <original-repo> rev-parse --verify --end-of-options <ref>^{commit}
```

Resolve each side independently and require a full 40- or 64-character
hexadecimal commit ID. Do not fetch, update refs, inspect index/worktree diffs,
or include uncommitted changes. Resolve the comparison base with:

```text
git -C <original-repo> merge-base <base-sha> <head-sha>
```

A missing merge base is a hard preflight error. The actual comparison is
merge-base-to-head, not base-to-head. Capture both status and textual hunks
before creating any output or worktree:

```text
git -C <original-repo> diff --find-renames --name-status -z <merge-base-sha> <head-sha>
git -C <original-repo> diff --find-renames --numstat -z <merge-base-sha> <head-sha>
git -C <original-repo> diff --find-renames --unified=0 <merge-base-sha> <head-sha> --
```

Parse NUL-delimited status records without decoding file paths through shell
word splitting. A rename record has an old and a new path; retain both. Use the
zero-context patch to map changed new-side line ranges and preserve removed
lines. `numstat` marks binary files with `-` counts. If the diff has no changed
paths, stop. If it has no readable text hunks (including a binary-only or
submodule-only range), list the affected paths and stop before creating the
bundle or worktree. A mixed range may proceed when its text hunks provide
source-grounded evidence; identify binary paths as non-text evidence.

The immutable range ID is `<merge-base-full-sha>..<head-full-sha>`, using the
full lowercase IDs returned by Git. Check both target paths before creating
anything:

```text
<original-repo>/.repowiki/changes/<range-id>
<original-repo>/.repowiki/.worktrees/<range-id>/<original-repo-basename>
```

If either exists, refuse this invocation without modifying it. Never reuse or
overwrite a previous edition. Create parents only after all ref/diff/path
preflight checks pass.

## Detached analysis worktree and session ownership

Check the head commit's tree for mode `160000` entries with
`git ls-tree -r -z <head-sha>`. If it contains submodules, add the worktree
with `git worktree add --detach --recurse-submodules <worktree> <head-sha>`;
otherwise use `git worktree add --detach <worktree> <head-sha>`. Run Git through
argv arrays, and verify the worktree is at the exact resolved head SHA. Its
last path segment must be the original repository basename, so analysis state
and Reader metadata keep the real project title.

Run a fresh, full dependency analysis, with no `--include`, `--focus`, or
`generate --update`:

```text
scripts/codewiki --repo-root <original-repo> generate --repo <worktree> --output <change-bundle> --max-depth 2
```

`--repo` identifies source at the head worktree; `--output` identifies the
new change edition in the original checkout. `--repo-root` is the effective
session-storage root. Keep it on every later session command, including
`components read`, `prompt get`, tree commands, document commands, validation,
and `session close`. The session must be under
`<original-repo>/.repowiki/.codewiki/sessions/<session-id>/`, while its
`state.json` `repo_path` remains the worktree. Locks are under the sibling
`session-locks/` directory. Never migrate or read root `.codewiki` sessions.

Capture the set of session IDs before generation. Save the returned session ID
and verify the reported session path and analyzed repository before continuing.
If generation fails after creating a session, keep every newly created
session for diagnosis and report its ID when identifiable.

## Change evidence and tree

Read the generated session's `component_index.json`, `summary.json`,
and dependency graph. Match each new-side changed line interval against
component `start_line`/`end_line` spans for that exact file. Record exact IDs
from the index; do not infer IDs from symbol text. Save the changed-ID set
separately from context IDs. `components read` may load exact source for changed
IDs and the limited unchanged neighbors needed to explain calls, dependencies,
or impact. Only changed IDs can become final tree/page anchors.

For deleted files, deleted lines, and the old side of a rename, use the saved
diff and source from `<merge-base-sha>`; never claim that old source exists at
head. Keep text changes that have no component mapping as path-level evidence.
Do not manufacture component IDs for configuration, prose, or deleted code.

Request repository `cluster` with `scope=repo`, changed component IDs, and a
change-specific `custom_instructions` value. It must partition only changed
responsibilities into reader-meaningful modules and may select representative
changed IDs. Keep unchanged neighbors in `architecture_context` or explicitly
scoped supporting source, not in the input ID file. Apply the host response
with `tree apply-cluster`, then recursively audit broad modules using
`scope=module`, exact parent component IDs, a parent-path JSON string array,
and `decomposition_review` for every response. Apply each response through the
CLI. Save the first tree, refine it, then save the final tree with
`--require-decomposition-review`; run `tree order` and use only its canonical
`doc_path` values.

The shared change-only instructions are:

> Explain only responsibilities changed between the resolved merge base and
> head. Use base-side diff/source to describe before and head-side source to
> describe after. Unchanged connected components are explanatory context for
> relationships, callers, dependencies, and impact; never promote them to
> change-tree anchors. Do not describe unrelated repository behavior. Every
> selected component ID must be copied from the changed-ID input.

Pass these instructions as `custom_instructions` to both repository/module
`cluster` prompts and to `overview_repo`/`overview_module`. For leaf and complex
pages, pass them to `system_leaf`/`system_complex`; the `user` prompt remains
unchanged. A page worker receives only its page's relevant before/after diff,
current component source, necessary unchanged-neighbor source, canonical page
path, and complete role-matched examples. Never send every file or the full
repository to every page worker.

If readable text changes exist but no current head component IDs exist (for
example, a deletion-only change), skip `tree apply-cluster`. Build a single
meaningful change leaf with `components: []`, `children: {}`, and a
`retain_leaf` decomposition review that cites the deleted path-level evidence;
save it directly through `tree save`, still requiring a decomposition review.
Do not run the normal changed-ID clustering with an empty input file. Binary-
only changes without text evidence stop before bundle creation.

## Pages and validation

Follow `tree order` in dependency order: leaves first, parent overviews after
child pages, repository `overview.md` last. Use `system_leaf` plus `user` for
leaf modules, `system_complex` for selected complex modules,
`overview_module` for parents, and `overview_repo` for `overview.md`. Select
complete examples from `references/few-shots/` matching the role; do not send
irrelevant examples. Preserve the existing page length, source-anchor, child
link, local-link, and Mermaid checks. The change overview's `repo_structure`
contains the invocation refs, resolved base/head/merge-base SHAs, file status,
changed component IDs, and final change tree. Its `architecture_context`
retains the complete head dependency graph. Mention before/after behavior and
source evidence only where supported by the patch and retrieved sources.

Write Markdown through `doc write` using the exact `doc_path` from `tree order`.
Run `doc validate` after all pages exist. For a failing page, give one fresh
repair worker only that page's evidence and diagnostics; retry once and
validate again. Do not close a session with an invalid report. Close a
successful session through `session close` before removing the worktree.

## Failure and cleanup

The bundle and worktree target paths were absent at preflight, so this
invocation owns anything newly created at those exact paths. On failure after
creation:

1. Preserve the session directory and all session-side diagnostics. Report the
   session ID (or the new session ID discovered relative to the pre-run
   snapshot).
2. Remove only this invocation's new change bundle. Do not touch `.repowiki/`
   root files, another change range, pre-existing worktrees, or caller-selected
   output paths.
3. Remove only this invocation's detached worktree with `git worktree remove`
   (force only when needed to remove the known clean detached checkout). If Git
   never registered it, remove its exact path only after verifying it is the
   invocation-created path.
4. Report the failing command, validation paths, and preserved session ID.

On success, `session close` performs normal session cleanup; then remove the
worktree. Keep the generated bundle. If session close fails, treat the run as
failed, preserve its session, and follow failure cleanup. The original
`.repowiki/` repository bundle and all old root `.codewiki` data remain
untouched on every path.
