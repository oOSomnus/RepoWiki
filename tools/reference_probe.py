#!/usr/bin/env python3
"""Run the reference parser and emit a small, stable differential contract.

This adapter is deliberately separate from the core replay.  It imports the
reference implementation only when the explicit ``test-reference`` target is
requested, so missing reference dependencies can be reported as a hard,
actionable failure without weakening the offline Skill gate.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any


LANGUAGE_BY_SUFFIX = {
    ".c": "C",
    ".cc": "C++",
    ".cpp": "C++",
    ".cxx": "C++",
    ".cs": "C#",
    ".java": "Java",
    ".js": "JavaScript",
    ".jsx": "JavaScript",
    ".kt": "Kotlin",
    ".kts": "Kotlin",
    ".php": "PHP",
    ".py": "Python",
    ".rb": "Ruby",
    ".scala": "Scala",
    ".ts": "TypeScript",
    ".tsx": "TypeScript",
}
EXPECTED_LANGUAGES = sorted(
    {
        "C",
        "C#",
        "C++",
        "Java",
        "JavaScript",
        "Kotlin",
        "PHP",
        "Python",
        "Ruby",
        "Scala",
        "TypeScript",
    }
)


class ProbeFailure(RuntimeError):
    """A reference adapter failure that should fail the differential gate."""


def normalize_relative_path(value: str) -> str:
    path = Path(value).as_posix()
    return path[2:] if path.startswith("./") else path


def language_for(node: Any) -> str:
    observed = getattr(node, "language", None)
    if observed:
        aliases = {
            "c": "C",
            "c++": "C++",
            "cpp": "C++",
            "c#": "C#",
            "csharp": "C#",
            "java": "Java",
            "javascript": "JavaScript",
            "js": "JavaScript",
            "kotlin": "Kotlin",
            "php": "PHP",
            "python": "Python",
            "ruby": "Ruby",
            "scala": "Scala",
            "typescript": "TypeScript",
            "ts": "TypeScript",
        }
        return aliases.get(str(observed).lower(), str(observed))
    relative_path = normalize_relative_path(str(getattr(node, "relative_path", "")))
    suffix = Path(relative_path).suffix.lower()
    try:
        return LANGUAGE_BY_SUFFIX[suffix]
    except KeyError as exc:
        raise ProbeFailure(
            f"reference node {getattr(node, 'id', '<unknown>')} has no language and "
            f"an unsupported suffix: {relative_path}"
        ) from exc


def canonical_components(components: dict[str, Any]) -> dict[str, Any]:
    result = []
    for component_id, node in components.items():
        relative_path = normalize_relative_path(str(node.relative_path))
        result.append(
            {
                "id": str(component_id),
                "name": str(node.name),
                "component_type": str(node.component_type),
                "relative_path": relative_path,
                "language": language_for(node),
                "start_line": int(node.start_line),
                "end_line": int(node.end_line),
                "depends_on": sorted(str(dep) for dep in node.depends_on),
            }
        )
    result.sort(key=lambda item: item["id"])
    paths = sorted({item["relative_path"] for item in result})
    languages = sorted({item["language"] for item in result})
    if languages != EXPECTED_LANGUAGES:
        raise ProbeFailure(
            f"reference language coverage mismatch: expected {EXPECTED_LANGUAGES}, got {languages}"
        )
    if len(paths) != len(EXPECTED_LANGUAGES):
        raise ProbeFailure(
            f"reference supported-file count mismatch: expected {len(EXPECTED_LANGUAGES)}, "
            f"got {len(paths)} ({paths})"
        )
    return {
        "summary": {
            "languages": languages,
            "supported_files": len(paths),
            "total_components": len(result),
        },
        "components": result,
        "leaf_nodes": [item["id"] for item in result if not item["depends_on"]],
        "dependencies": {
            item["id"]: item["depends_on"] for item in result
        },
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reference-root", type=Path, required=True)
    parser.add_argument("--repo", type=Path, required=True)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    reference_root = args.reference_root.resolve()
    sys.path.insert(0, str(reference_root))
    try:
        from codewiki.src.be.dependency_analyzer.ast_parser import DependencyParser
    except ModuleNotFoundError as exc:
        missing = exc.name or str(exc)
        print(
            "REFERENCE_DEPENDENCIES_MISSING: cannot import the reference parser; "
            f"missing module {missing!r}. Install reference/CodeWiki dependencies before "
            "running test-reference.",
            file=sys.stderr,
        )
        return 2
    except Exception as exc:  # pragma: no cover - depends on external environment
        print(
            "REFERENCE_IMPORT_FAILED: reference parser import raised "
            f"{type(exc).__name__}: {exc}",
            file=sys.stderr,
        )
        return 2

    try:
        parser = DependencyParser(
            str(args.repo.resolve()),
            use_gitignore=True,
            artifact_options=None,
        )
        components = parser.parse_repository()
        result = canonical_components(components)
    except Exception as exc:
        print(
            f"REFERENCE_PARSE_FAILED: {type(exc).__name__}: {exc}",
            file=sys.stderr,
        )
        return 1

    print(json.dumps(result, ensure_ascii=False, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
