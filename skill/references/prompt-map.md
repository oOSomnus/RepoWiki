# Prompt map

Prompt bodies are embedded in the bundled Rust executable. The CLI validates
the variable object before substitution and writes the rendered prompt to the
session workspace. The prompt list command returns both prompt_types and
prompt_specs.

The vars-file must contain a JSON object. Required variables must be present
and non-null; unknown variables are rejected. The cluster scope defaults to
repository mode and accepts only repo or module.

## Runtime catalog

The runtime catalog contains these prompt types:

cluster, super_group, filter_folders, system_complex, system_leaf, user, overview_module, overview_repo,
update_leaf_system, update_leaf_user, routing_system, routing_user,
stale_fix_system, stale_fix_user.

## Generation prompts

| Type | Required variables | Optional or conditional variables |
|---|---|---|
| cluster | one of potential_core_components or component_ids | scope; when scope=module, also module_name and module_tree |
| super_group | formatted_modules | none |
| filter_folders | project_name, files | none |
| system_complex | module_name | custom_instructions, few_shot_examples |
| system_leaf | module_name | custom_instructions, few_shot_examples |
| user | module_name, module_tree, and one of formatted_core_component_codes or component_ids | artifact_index, few_shot_examples, architecture_context |
| overview_module | module_name, repo_structure | few_shot_examples, architecture_context |
| overview_repo | repo_name, repo_structure | artifact_index, few_shot_examples, architecture_context |

The cluster response uses GROUPED_COMPONENTS or GROUPED_MODULES markers. Those
are response-format markers, not input variable names. In particular,
grouped_components, grouped_modules, and core_component_codes must not be
substituted for potential_core_components or
formatted_core_component_codes.

When `component_ids` is supplied, the CLI resolves the IDs from the session,
groups them by file, marks artifact files, inlines readable source, and renders
the module tree as an indented outline. `artifact_index` adds the artifact
usage note and index to documentation prompts.

Use `scope=repo` for the first semantic partition and `scope=module` for every
recursive refinement. Module refinement receives the current tree and exact
parent component IDs; it returns child groups while the parent retains its
aggregate component list. A prompt response is not a license to stop
recursion: the saved tree's quality diagnostics decide whether a leaf fits.

## Update prompts

| Type | Required variables | Optional variables |
|---|---|---|
| update_leaf_system | leaf_name | custom_instructions |
| update_leaf_user | leaf_name, mode, mode_note, write_set, report, module_tree, leaf_components, leaf_page | none |
| routing_system | none | none |
| routing_user | module_tree, orphans | none |
| stale_fix_system | none | none |
| stale_fix_user | page, items | none |

Use the update prompts only with the reports and write sets produced by the
update workflow. Preserve component IDs and the required fenced JSON verdict
markers in Agent responses.

## Size and Unicode

User and cluster prompts are capped at 900000 Unicode scalar characters. The
CLI truncates only at valid UTF-8 character boundaries and reports the
character count, not the byte count. Do not implement a second byte-based
truncation layer in the host agent.

Do not compare generated prose byte-for-byte. Preserve the prompt type,
required variables, response markers, component IDs, JSON verdict block, and
module-tree shape.
