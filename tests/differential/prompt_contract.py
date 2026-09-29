#!/usr/bin/env python3
"""Check the architecture-reading prompt contract.

RepoWiki is intentionally not a compatibility implementation of the pinned
reference host. The reference material is used as a small set of few-shot
architecture examples; this gate checks the current prompt contract and the
examples shipped with the Skill instead of comparing prompt implementations.
"""

from __future__ import annotations

import hashlib
import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
CURRENT = ROOT / "engine" / "prompts"
FEW_SHOTS = ROOT / "skill" / "references" / "few-shots"


class PromptContractFailure(RuntimeError):
    pass


def require(text: str, fragments: list[str], label: str) -> None:
    missing = [fragment for fragment in fragments if fragment not in text]
    if missing:
        raise PromptContractFailure(f"{label} is missing: {missing}")


def change_skill_contract() -> None:
    skill_dir = ROOT / "change-wiki"
    skill_text = (skill_dir / "SKILL.md").read_text(encoding="utf-8")
    require(
        skill_text,
        [
            "name: change-wiki",
            "/change-wiki <base-ref>...<head-ref>",
            ".repowiki/changes/",
            "references/change-workflow.md",
            "PHP 8.2 or newer",
            "change_<SHA256(complete base..head ID)>",
            "does not support old",
        ],
        "change-wiki/SKILL.md",
    )
    agent_text = (skill_dir / "agents" / "openai.yaml").read_text(encoding="utf-8")
    require(
        agent_text,
        ["allow_implicit_invocation: false"],
        "change-wiki/agents/openai.yaml",
    )
    workflow_path = skill_dir / "references" / "change-workflow.md"
    workflow_text = workflow_path.read_text(encoding="utf-8")
    require(
        workflow_text,
        [
            "rev-parse --verify --end-of-options",
            "merge-base",
            "diff --find-renames --name-status -z",
            "diff --find-renames --unified=0",
            "Do not fetch",
            ".repowiki/.codewiki/sessions/",
            "components: []",
            "change_<SHA256(complete base..head ID)>:",
            "<mermaid>...</mermaid>",
            "does not support old",
        ],
        "change-wiki/references/change-workflow.md",
    )

    shared_files = [
        "references/cli-contract.md",
        "references/prompt-map.md",
        "references/few-shots/README.md",
        "references/few-shots/clickhouse-overview.md",
        "references/few-shots/clickhouse-storage-engine.md",
        "references/few-shots/clickhouse-query-pipeline.md",
        "references/few-shots/clickhouse-ast-create-query.md",
    ]
    for relative in shared_files:
        repo_copy = (ROOT / "skill" / relative).read_bytes()
        change_copy = (skill_dir / relative).read_bytes()
        if repo_copy != change_copy:
            raise PromptContractFailure(
                f"change-wiki/{relative} differs from the RepoWiki source reference"
            )


def main() -> int:
    current = {
        path.name: path.read_text(encoding="utf-8")
        for path in CURRENT.glob("*.txt")
    }
    required_current = {
        "cluster_repo.txt": [
            "architecture map",
            "representative component IDs",
            "directory classification task",
            "decomposition_review",
            "breadth_risk",
            "two to four",
            "<GROUPED_COMPONENTS>",
            "DokuWiki namespace segments",
            "repo:system:api:start",
            ".txt",
        ],
        "cluster_module.txt": [
            "existing architecture module",
            "representative component IDs",
            "<DECOMPOSITION_REVIEW>",
            "retain_leaf",
            "high-risk leaf",
            "directory-shaped child",
            "Depth is earned",
            "Return one child level per response",
            "{current_depth}",
            "{remaining_depth}",
            "<GROUPED_COMPONENTS>",
            "DokuWiki namespace segments",
            "repo:system:api:start",
        ],
        "super_group.txt": [
            "<MODULES>",
            "<GROUPED_MODULES>",
            "Preserve the existing module pages",
            "adds one level",
        ],
        "user.txt": [
            "<MODULE_TREE>",
            "<ARCHITECTURE_CONTEXT>",
            "<ARCHITECTURE_FEW_SHOTS>",
            "source-grounded architecture page",
            "Caller -> Entry -> Core -> Result",
            "native DokuWiki source",
            "repo:system:api:start",
        ],
        "overview_module.txt": [
            "<REPO_STRUCTURE>",
            "<ARCHITECTURE_CONTEXT>",
            "<ARCHITECTURE_FEW_SHOTS>",
            "every documented immediate child page",
            "Caller -> Entry -> Core -> Result",
            "<mermaid>...</mermaid>",
        ],
        "overview_repo.txt": [
            "<REPO_STRUCTURE>",
            "<ARCHITECTURE_CONTEXT>",
            "<ARCHITECTURE_FEW_SHOTS>",
            "primary Mermaid architecture diagram",
            "end-to-end path",
            "generic placeholders",
            "native DokuWiki page source",
        ],
        "system_leaf.txt": [
            "architecture documentation writer",
            "codewiki doc write",
            "Mermaid",
            "substantive article",
            "two distinct source anchors",
            "complete reference article",
            "native DokuWiki source",
            "repo:system:api:start",
            "<mermaid>...</mermaid>",
        ],
        "system_complex.txt": [
            "architecture documentation writer",
            "already-selected architecture module",
            "codewiki doc write",
            "selected components form one module",
            "two distinct anchors",
            "complete reference article",
            "fixed headings",
            "canonical DokuWiki page ID",
            "repo:system:api:start",
        ],
        "filter_folders.txt": ["relative paths", "shortlist", "JSON format"],
        "update_leaf_user.txt": [
            "<WRITE_SET>",
            "<CHANGE_REPORT>",
            "<LEAF_COMPONENTS>",
            "verdict",
        ],
        "update_leaf_system.txt": [
            "WRITE SET",
            "DokuWiki syntax",
            "canonical page ID",
            "repo:system:api:start",
        ],
        "routing_system.txt": ["existing module tree", "JSON only"],
        "routing_user.txt": [
            "<MODULE_TREE>",
            "<ORPHANS>",
            "canonical page ID",
        ],
        "stale_fix_system.txt": [
            "stale references",
            "verdicts",
            "canonical-page-id",
        ],
        "stale_fix_user.txt": [
            "<STALE_ITEMS>",
            "codewiki doc view",
            "codewiki doc edit",
            "complete canonical DokuWiki page ID",
        ],
    }
    expected_prompt_files = {
        "artifact_usage.txt",
        "cluster_module.txt",
        "cluster_repo.txt",
        "code_truncated.txt",
        "filter_folders.txt",
        "module_tree_trimmed.txt",
        "overview_artifact_addendum.txt",
        "overview_module.txt",
        "overview_repo.txt",
        "routing_system.txt",
        "routing_user.txt",
        "stale_fix_system.txt",
        "stale_fix_user.txt",
        "super_group.txt",
        "system_complex.txt",
        "system_leaf.txt",
        "update_leaf_system.txt",
        "update_leaf_user.txt",
        "user.txt",
    }
    if set(current) != expected_prompt_files:
        raise PromptContractFailure(
            "current prompt catalog differs: "
            f"expected {sorted(expected_prompt_files)}, got {sorted(current)}"
        )
    for name, fragments in required_current.items():
        text = current.get(name)
        if text is None:
            raise PromptContractFailure(f"missing current prompt source {name}")
        require(text, fragments, f"current/{name}")
        for legacy in (
            "str_replace_editor",
            "read_code_components",
            "generate_sub_module_documentation",
        ):
            if legacy in text:
                raise PromptContractFailure(f"current/{name} contains legacy tool {legacy}")

    expected_few_shots = {
        "README.md",
        "clickhouse-overview.md",
        "clickhouse-storage-engine.md",
        "clickhouse-query-pipeline.md",
        "clickhouse-ast-create-query.md",
    }
    actual_few_shots = {path.name for path in FEW_SHOTS.iterdir() if path.is_file()}
    if actual_few_shots != expected_few_shots:
        raise PromptContractFailure(
            "architecture few-shot catalog differs: "
            f"expected {sorted(expected_few_shots)}, got {sorted(actual_few_shots)}"
        )
    require(
        (FEW_SHOTS / "README.md").read_text(encoding="utf-8"),
        [
            "clickhouse-overview.md",
            "clickhouse-storage-engine.md",
            "native DokuWiki page syntax",
            "9dc8cf8c41705960f2002f3489a6dc302c936114",
        ],
        "few-shots/README.md",
    )
    skill_text = (ROOT / "skill" / "SKILL.md").read_text(encoding="utf-8")
    require(
        skill_text,
        [
            "Recursively review modules",
            "current_depth",
            "remaining_depth",
            "--require-decomposition-review",
            "one new worker with isolated context per DokuWiki page",
            "at most four workers",
            "fresh reviewer/repair worker",
            "PHP 8.2 or newer",
            "repo:start",
            "<mermaid>...</mermaid>",
            ".txt",
        ],
        "skill/SKILL.md",
    )
    change_skill_contract()
    expected_hashes = {
        "clickhouse-overview.md": "98f7f367469fa3a07e5fa57b2a9e61bf04114b9c215398b0f1db1903dd0aa874",
        "clickhouse-storage-engine.md": "45ed25706cc2cfcbd217a6c6f51499131a0e226423bd4d61fed391c1c640562b",
        "clickhouse-query-pipeline.md": "c70ce5d1b6b6f337d90accd477963979e4afdfe85bf9bcbffe3ebbde4d2f219a",
        "clickhouse-ast-create-query.md": "83e0fad779dc097bba8ea35627ba6599a92a09a53a21b7dd226f4ec8eb0579dd",
    }
    for path in FEW_SHOTS.glob("*.md"):
        if path.name == "README.md":
            continue
        text = path.read_text(encoding="utf-8")
        require(text.lower(), ["architecture", "mermaid"], f"few-shots/{path.name}")
        require(text, ["======", "[[repo:", "<mermaid>"], f"few-shots/{path.name}")
        if re.search(r"(?m)^#{1,6}\s|```|\[[^\]]+\]\([^)]+\)", text):
            raise PromptContractFailure(
                f"{path.name} still contains Markdown page syntax"
            )
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        if digest != expected_hashes.get(path.name):
            raise PromptContractFailure(
                f"{path.name} is not the native DokuWiki reference source: {digest}"
            )

    print(
        "PASS architecture prompt contract: "
        f"{len(required_current)}/{len(current)} current prompts, "
        f"{len(expected_few_shots) - 1} reference-derived few shots"
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, PromptContractFailure) as exc:
        print(f"PROMPT CONTRACT FAILED: {exc}")
        raise SystemExit(1)
