#!/usr/bin/env python3
"""Static semantic prompt checks shared by the offline and reference gates.

The reference implementation and RepoWiki intentionally use different host
tools.  This check compares the obligations that affect the wiki workflow:
module recursion, exact IDs, artifact awareness, overview structure and
machine-readable responses.  It deliberately does not compare prose bytes.
"""

from __future__ import annotations

import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
CURRENT = ROOT / "engine" / "prompts"
REFERENCE = ROOT / "reference" / "CodeWiki" / "codewiki" / "src" / "be"
COMPATIBILITY = ROOT / "tests" / "contracts" / "reference-compatibility.json"


class PromptContractFailure(RuntimeError):
    pass


def require(text: str, fragments: list[str], label: str) -> None:
    missing = [fragment for fragment in fragments if fragment not in text]
    if missing:
        raise PromptContractFailure(f"{label} is missing: {missing}")


def main() -> int:
    compatibility = json.loads(COMPATIBILITY.read_text(encoding="utf-8"))
    if len(compatibility["common_analysis_languages"]) != 11:
        raise PromptContractFailure("common reference language allowlist changed unexpectedly")

    current = {
        path.name: path.read_text(encoding="utf-8")
        for path in CURRENT.glob("*.txt")
    }
    required_current = {
        "cluster_repo.txt": [
            "<POTENTIAL_CORE_COMPONENTS>",
            "<GROUPED_COMPONENTS>",
            "repository-level clustering pass",
            "scope=module",
            "exactly one",
        ],
        "cluster_module.txt": [
            "<MODULE_TREE>",
            "recursive refinement pass",
            "child groups only",
            "parent keeps its aggregate component list",
            "<GROUPED_COMPONENTS>",
        ],
        "super_group.txt": [
            "<MODULES>",
            "<GROUPED_MODULES>",
            "Preserve the existing module pages",
        ],
        "user.txt": ["<MODULE_TREE>", "<CORE_COMPONENT_CODES>", ".md"],
        "overview_module.txt": ["<REPO_STRUCTURE>", "docs_path", "<OVERVIEW>"],
        "overview_repo.txt": ["<REPO_STRUCTURE>", "docs_path", "<OVERVIEW>"],
        "system_leaf.txt": ["codewiki components read", "codewiki doc write"],
        "system_complex.txt": ["already-clustered module", "codewiki doc write"],
        "filter_folders.txt": ["relative paths", "shortlist", "JSON format"],
        "update_leaf_user.txt": ["<WRITE_SET>", "<CHANGE_REPORT>", "<LEAF_COMPONENTS>", "verdict"],
        "routing_system.txt": ["existing module tree", "JSON only"],
        "routing_user.txt": ["<MODULE_TREE>", "<ORPHANS>", "decisions"],
        "stale_fix_system.txt": ["stale references", "verdicts"],
        "stale_fix_user.txt": ["<STALE_ITEMS>", "codewiki doc view", "codewiki doc edit"],
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

    reference_template = (REFERENCE / "prompt_template.py").read_text(encoding="utf-8")
    require(
        reference_template,
        [
            "CLUSTER_REPO_PROMPT",
            "CLUSTER_MODULE_PROMPT",
            "SUPER_GROUP_PROMPT",
            "<GROUPED_COMPONENTS>",
            "<GROUPED_MODULES>",
            "Each component ID has the form",
            "REPO_OVERVIEW_PROMPT",
            "MODULE_OVERVIEW_PROMPT",
        ],
        "reference/prompt_template.py",
    )
    reference_updater = (REFERENCE / "updater" / "prompts.py").read_text(encoding="utf-8")
    require(
        reference_updater,
        ["WRITE_SET", "CHANGE_REPORT", "verdicts", "routing"],
        "reference/updater/prompts.py",
    )
    exact_rules = compatibility.get("exact_parity", [])
    if not isinstance(exact_rules, list) or not all(isinstance(rule, str) for rule in exact_rules):
        raise PromptContractFailure("compatibility exact_parity must be a list of strings")
    divergences = compatibility.get("intentional_divergence", [])
    if not isinstance(divergences, list) or not all(isinstance(item, str) for item in divergences):
        raise PromptContractFailure(
            "compatibility intentional_divergence must be a list of strings"
        )

    print(
        "PASS prompt semantic contract: "
        f"{len(required_current)}/{len(current)} current prompts, "
        f"{len(compatibility['exact_parity'])} exact parity rules, "
        f"{len(compatibility['intentional_divergence'])} documented divergences"
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, json.JSONDecodeError, PromptContractFailure) as exc:
        print(f"PROMPT CONTRACT FAILED: {exc}")
        raise SystemExit(1)
