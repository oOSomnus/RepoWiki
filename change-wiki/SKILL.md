---
name: change-wiki
description: Generate a source-grounded wiki for an explicit local Git range.
metadata:
  short-description: Wiki for an explicit Git change range
---

# Change-scope Wiki

Run only when the user explicitly invokes `/change-wiki <base-ref>...<head-ref>`.
Never infer a range or include staged, unstaged, or uncommitted work. This
Skill writes one immutable change edition at
`.repowiki/changes/<merge-base-full-SHA>..<head-full-SHA>/`; it does not replace
the repository edition at `.repowiki/`.

Read [references/change-workflow.md](references/change-workflow.md) before
running commands. It defines safe Git argument handling, preflight checks,
worktree/session ownership, evidence mapping, failure cleanup, and the full
page-generation sequence. Also read [references/cli-contract.md](references/cli-contract.md)
and [references/prompt-map.md](references/prompt-map.md).

## Workflow

1. Resolve the original repository root and parse exactly one explicit
   `base-ref...head-ref`. Follow the reference workflow to resolve both refs to
   local commits, compute the merge base, and collect rename-aware status plus
   text hunks. Do not fetch. Stop before creating output or a worktree if a ref
   is invalid, there is no merge base, the diff is empty, or it has no
   explainable text evidence.
2. Compute the full-SHA range ID and fixed paths under the original
   repository's `.repowiki/`. Refuse an existing change bundle or worktree
   path; never overwrite. Create a detached worktree at the exact head commit,
   using the original repository basename as its final directory name.
3. Run a fresh full analysis, without `--include`, `--focus`, or update mode:

   ```text
   scripts/codewiki --repo-root <original-repo> generate --repo <worktree> --output <change-bundle> --max-depth 2
   ```

   Keep the returned session ID. Confirm the analyzed `repo_path` is the
   worktree and the session lives at
   `<original-repo>/.repowiki/.codewiki/sessions/<session_id>/`. Prefix every
   later session command with the same `--repo-root <original-repo>`.
4. Map new-side diff hunk lines to exact component IDs using the session's
   `component_index.json` `start_line`/`end_line` spans. Keep the complete head
   dependency graph. Use changed component IDs as the only change-tree anchors;
   unchanged callers, callees, and neighboring modules are context only. For
   deletions and rename sources, use the patch and source from the merge-base
   commit. Preserve changed text paths as path-level evidence if no current
   component maps to them; never invent component IDs.
5. Use the existing clustering, recursive `scope=module` review, two-phase
   `tree save`, `tree order`, and page-writing flow. Require a
   `decomposition_review` for every final entry. For an all-deletion change
   with readable text evidence and no current component IDs, create one
   `components: []` leaf with a `retain_leaf` review and save it directly with
   `tree save`; `tree apply-cluster` drops empty groups. Do not create a bundle
   for binary-only changes without readable text evidence.
6. For cluster and overview prompts, pass optional `custom_instructions`; for
   leaf or complex page prompts, pass them to `system_leaf` or `system_complex`.
   Require every page to explain only change-owned responsibilities. Unchanged
   dependencies may explain relationships, callers, or impact, but must not
   become page anchors. Each page worker receives only its own diff, relevant
   current source, necessary unchanged-neighbor source, canonical page path,
   and complete matching few-shots. The repository overview receives the full
   range metadata, file statuses, changed component IDs, final tree, and the
   complete dependency graph as `architecture_context`.
7. Write every page through the CLI, then run `doc validate`. Repair a failing
   page at most once with its evidence and diagnostics, and validate again.
   Close the session only after validation succeeds. On success, remove the
   worktree and report the refs, resolved SHAs, bundle path, and validation
   result.

On any failure after creation, keep the session directory for diagnosis and
report its session ID. Remove only the change bundle and worktree created by
this invocation. Never remove the repository bundle, another range, old
`.codewiki` data, or caller-selected paths. See the reference workflow for the
exact rollback rules. The Reader can open the enclosing `.repowiki/` directory
to switch between its repository edition and change editions.
