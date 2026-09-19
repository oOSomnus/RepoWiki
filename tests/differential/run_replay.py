#!/usr/bin/env python3
"""Run the deterministic host-agent replay against a Skill binary.

This test intentionally uses only the standard library.  It exercises the
installed runtime contract through subprocesses and a fixed transcript; it
does not import the reference Python implementation, call an LLM, or use
MCP.  The optional ZIP path is extracted into a temporary Skill directory and
run separately from the preview directory.
"""

from __future__ import annotations

import argparse
import difflib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path
from typing import Any

try:
    from canonical import canonical_analysis, canonical_json, canonical_metadata, canonical_tree
except ImportError:  # pragma: no cover - supports ``python -m`` execution
    from tests.differential.canonical import (  # type: ignore[no-redef]
        canonical_analysis,
        canonical_json,
        canonical_metadata,
        canonical_tree,
    )


ROOT = Path(__file__).resolve().parents[2]
FIXTURE = Path(__file__).resolve().parent / "fixture"
TRANSCRIPT_PATH = Path(__file__).resolve().parent / "transcript.json"
GOLDEN_PATH = Path(__file__).resolve().parent.parent / "golden" / "mini-repo.json"
LANGUAGES = [
    "C",
    "C#",
    "C++",
    "Go",
    "Java",
    "JavaScript",
    "Kotlin",
    "PHP",
    "Python",
    "Ruby",
    "Rust",
    "Scala",
    "TypeScript",
]
RESERVED_DOCUMENT_STEMS = {
    "overview",
    "module_tree",
    "first_module_tree",
    "metadata",
    "index",
}


class ReplayFailure(RuntimeError):
    """A user-actionable failure in the public Skill contract."""


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ReplayFailure(f"cannot read JSON {path}: {exc}") from exc


def write_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(canonical_json(value), encoding="utf-8")


def run_command(binary: Path, args: list[str], *, cwd: Path | None = None) -> dict[str, Any]:
    command = [str(binary), *args]
    try:
        completed = subprocess.run(
            command,
            cwd=cwd,
            text=True,
            capture_output=True,
            check=False,
        )
    except OSError as exc:
        raise ReplayFailure(f"cannot execute {' '.join(command)}: {exc}") from exc
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip()
        raise ReplayFailure(
            f"command failed ({completed.returncode}): {' '.join(command)}\n{detail}"
        )
    try:
        value = json.loads(completed.stdout)
    except json.JSONDecodeError as exc:
        raise ReplayFailure(
            f"command did not return JSON: {' '.join(command)}\n{completed.stdout}"
        ) from exc
    if not isinstance(value, dict) or value.get("ok") is False:
        raise ReplayFailure(f"command returned an error: {' '.join(command)}\n{completed.stdout}")
    return value


def resolve_binary(location: Path) -> Path:
    if location.is_file():
        return location.resolve()
    for name in ("codewiki", "codewiki.exe"):
        candidate = location / "scripts" / name
        if candidate.is_file():
            return candidate.resolve()
    raise ReplayFailure(f"Skill binary not found under {location}")


def extract_archive(archive: Path, destination: Path) -> Path:
    if not archive.is_file():
        raise ReplayFailure(f"Skill archive does not exist: {archive}")
    with zipfile.ZipFile(archive) as package:
        members = package.infolist()
        for member in members:
            name = member.filename
            path = Path(name)
            if path.is_absolute() or ".." in path.parts:
                raise ReplayFailure(f"unsafe archive member: {name}")
            package.extract(member, destination)
            extracted = destination / path
            # Python's ZipFile.extract() intentionally does not restore Unix
            # mode bits.  Preserve the executable bit recorded by `zip` so
            # this is a real installed-binary smoke, not just a file check.
            mode = (member.external_attr >> 16) & 0o777
            if mode and extracted.is_file():
                extracted.chmod(mode)
    binary = resolve_binary(destination)
    if os.name != "nt" and not os.access(binary, os.X_OK):
        raise ReplayFailure(
            f"installed Skill binary is not executable after ZIP extraction: {binary}; "
            "the archive must preserve the executable mode"
        )
    return binary


def prompt_vars(
    prompt_type: str,
    tree: dict[str, Any],
    module_name: str,
    source_text: str,
) -> dict[str, Any]:
    tree_text = json.dumps(tree, ensure_ascii=False, indent=2, sort_keys=True)
    if prompt_type == "cluster":
        return {"scope": "repo", "potential_core_components": source_text}
    if prompt_type == "system_leaf":
        return {"module_name": module_name, "custom_instructions": "offline replay"}
    if prompt_type == "user":
        return {
            "module_name": module_name,
            "module_tree": tree_text,
            "formatted_core_component_codes": source_text,
        }
    if prompt_type == "overview_repo":
        return {"repo_name": "replay-repo", "repo_structure": tree_text}
    raise ReplayFailure(f"unsupported replay prompt type: {prompt_type}")


def get_prompt(
    binary: Path,
    repo: Path,
    session_id: str,
    prompt_type: str,
    variables: dict[str, Any],
    work: Path,
) -> str:
    variables_path = work / f"{prompt_type}-{len(list(work.glob('*.json')))}.json"
    write_json(variables_path, variables)
    result = run_command(
        binary,
        [
            "prompt",
            "get",
            "--repo-root",
            str(repo),
            "--session",
            session_id,
            "--type",
            prompt_type,
            "--vars-file",
            str(variables_path),
        ],
    )
    path = Path(result["path"])
    if not path.is_file():
        raise ReplayFailure(f"prompt path does not exist: {path}")
    return str(result["sha256"])


def assert_equal(label: str, actual: Any, expected: Any) -> None:
    if actual == expected:
        return
    actual_text = canonical_json(actual)
    expected_text = canonical_json(expected)
    diff = "".join(
        difflib.unified_diff(
            expected_text.splitlines(keepends=True),
            actual_text.splitlines(keepends=True),
            fromfile=f"expected/{label}",
            tofile=f"actual/{label}",
        )
    )
    raise ReplayFailure(f"{label} differs:\n{diff}")


def expected_component_ids(tree: dict[str, Any]) -> list[str]:
    ids: list[str] = []
    for module in tree.values():
        ids.extend(module.get("components", []))
    return sorted(ids)


def document_path_for(module_name: str) -> str:
    """Mirror the documented module-page naming convention for new runtimes."""

    stem = "".join(
        character
        if character.isascii() and (character.isalnum() or character in "_-&")
        else "_"
        for character in module_name
    ) or "module"
    if stem in RESERVED_DOCUMENT_STEMS:
        stem = f"{stem}_module"
    return f"{stem}.md"


def execute_replay(binary: Path, transcript: dict[str, Any], root: Path) -> dict[str, Any]:
    repo = root / "repo"
    output = root / "output"
    shutil.copytree(FIXTURE, repo)
    output.mkdir()
    work = root / "work"
    work.mkdir()

    analysis = run_command(
        binary,
        ["generate", "--repo", str(repo), "--output", str(output)],
    )
    session_id = str(analysis["session_id"])
    session = Path(analysis["session_path"])
    if not session.is_dir():
        raise ReplayFailure(f"analysis session does not exist: {session}")

    try:
        summary = analysis["summary"]
        if sorted(summary["languages"]) != LANGUAGES:
            raise ReplayFailure(
                f"13-language coverage mismatch: expected {LANGUAGES}, got {summary['languages']}"
            )
        if summary["supported_files"] != len(LANGUAGES):
            raise ReplayFailure(
                f"ignored-file filtering mismatch: expected {len(LANGUAGES)} supported files, "
                f"got {summary['supported_files']}"
            )
        if summary["total_components"] != len(LANGUAGES):
            raise ReplayFailure(
                f"fixture component count mismatch: expected {len(LANGUAGES)}, "
                f"got {summary['total_components']}"
            )

        component_index = load_json(Path(analysis["component_index_path"]))
        leaf_nodes = load_json(Path(analysis["leaf_nodes_path"]))
        graph = load_json(Path(analysis["graph_path"]))
        if expected_component_ids(transcript["module_tree"]) != sorted(
            item["id"] for item in component_index
        ):
            raise ReplayFailure("transcript module tree does not cover the analyzed components")
        if "MustNotBeAnalyzed" in json.dumps(component_index):
            raise ReplayFailure(".gitignore fixture leaked into the component index")

        analysis_contract = canonical_analysis(
            analysis,
            component_index,
            leaf_nodes,
            graph,
            repo,
            output,
            session,
        )

        prompt_hashes: dict[str, list[str]] = {"cluster": []}
        prompt_hashes["cluster"].append(
            get_prompt(
                binary,
                repo,
                session_id,
                "cluster",
                prompt_vars(
                    "cluster", transcript["module_tree"], "Repository", "fixed cluster input"
                ),
                work,
            )
        )

        tree_path = work / "module-tree.json"
        write_json(tree_path, transcript["module_tree"])
        saved = run_command(
            binary,
            [
                "tree",
                "save",
                "--repo-root",
                str(repo),
                "--session",
                session_id,
                "--tree-file",
                str(tree_path),
                "--first",
            ],
        )
        save_result = saved["result"]
        if save_result["unmatched_component_ids"]:
            raise ReplayFailure(f"transcript tree has unmatched IDs: {save_result}")

        ids_path = work / "component-ids.json"
        write_json(ids_path, expected_component_ids(transcript["module_tree"]))
        components_result = run_command(
            binary,
            [
                "components",
                "read",
                "--repo-root",
                str(repo),
                "--session",
                session_id,
                "--ids-file",
                str(ids_path),
            ],
        )
        source_by_id: dict[str, str] = {}
        for item in components_result["components"]:
            source_path = Path(item["path"])
            if not source_path.is_file():
                raise ReplayFailure(f"component source path does not exist: {source_path}")
            source_by_id[item["id"]] = source_path.read_text(encoding="utf-8")

        ordered = run_command(
            binary,
            ["tree", "order", "--repo-root", str(repo), "--session", session_id],
        )["processing_order"]
        expected_documents = transcript["documents"]
        prompt_hashes.update({"system_leaf": [], "user": []})
        for item in ordered:
            module_name = item.get("module_name", item.get("module"))
            if not module_name:
                raise ReplayFailure(f"processing order item has no module name: {item}")
            doc_path = item.get("doc_path", document_path_for(module_name))
            ids = item["components"]
            source_text = "\n".join(source_by_id[component_id] for component_id in ids)
            variables = prompt_vars(
                "system_leaf", transcript["module_tree"], module_name, source_text
            )
            prompt_hashes["system_leaf"].append(
                get_prompt(binary, repo, session_id, "system_leaf", variables, work)
            )
            variables = prompt_vars(
                "user", transcript["module_tree"], module_name, source_text
            )
            prompt_hashes["user"].append(
                get_prompt(binary, repo, session_id, "user", variables, work)
            )
            if module_name not in expected_documents:
                raise ReplayFailure(f"transcript has no document for module {module_name}")
            content_path = work / f"{doc_path}.content"
            content_path.write_text(expected_documents[module_name], encoding="utf-8")
            run_command(
                binary,
                [
                    "doc",
                    "write",
                    "--repo-root",
                    str(repo),
                    "--session",
                    session_id,
                    "--path",
                    doc_path,
                    "--content-file",
                    str(content_path),
                ],
            )

        prompt_hashes["overview_repo"] = [
            get_prompt(
                binary,
                repo,
                session_id,
                "overview_repo",
                prompt_vars(
                    "overview_repo",
                    transcript["module_tree"],
                    "Repository",
                    "fixed overview input",
                ),
                work,
            )
        ]
        overview_path = work / "overview.md.content"
        overview_path.write_text(transcript["overview"], encoding="utf-8")
        run_command(
            binary,
            [
                "doc",
                "write",
                "--repo-root",
                str(repo),
                "--session",
                session_id,
                "--path",
                "overview.md",
                "--content-file",
                str(overview_path),
            ],
        )

        validation = load_json(session / "module_tree_validation.json")
        processing_order = load_json(session / "processing_order.json")
        closed = run_command(
            binary,
            [
                "session",
                "close",
                "--repo-root",
                str(repo),
                "--session",
                session_id,
                "--model",
                "replay-model",
            ],
        )
        if session.exists():
            raise ReplayFailure(f"session was not cleaned after close: {session}")
        metadata = load_json(output / "metadata.json")
        documents = {
            path.relative_to(output).as_posix(): path.read_text(encoding="utf-8")
            for path in output.rglob("*.md")
        }
        output_files = sorted(
            path.relative_to(output).as_posix()
            for path in output.rglob("*")
            if path.is_file()
        )
        replay = {
            "analysis": analysis_contract,
            "module_tree": canonical_tree(load_json(output / "module_tree.json")),
            "first_module_tree": canonical_tree(load_json(output / "first_module_tree.json")),
            "processing_order": processing_order,
            "validation": {
                "valid": validation["valid"],
                "unmatched_component_ids": validation["unmatched_component_ids"],
                "leftover_candidate_ids": validation["leftover_candidate_ids"],
                "module_count": validation["module_count"],
                "leaf_count": validation["leaf_count"],
            },
            "prompt_hashes": prompt_hashes,
            "documents": documents,
            "metadata": canonical_metadata(metadata, repo, output),
            "output_files": output_files,
            "close": {
                "cleaned": closed["cleaned"],
                "metadata_present": closed["metadata"] is not None,
            },
        }
        return replay
    finally:
        if session.exists():
            try:
                run_command(
                    binary,
                    [
                        "session",
                        "close",
                        "--repo-root",
                        str(repo),
                        "--session",
                        session_id,
                        "--model",
                        "replay-cleanup",
                    ],
                )
            except ReplayFailure:
                pass


def run(binary: Path, archive: Path | None, update_golden: bool) -> None:
    transcript = load_json(TRANSCRIPT_PATH)
    with tempfile.TemporaryDirectory(prefix="codewiki-replay-") as temporary:
        temporary_root = Path(temporary)
        preview_result = execute_replay(binary, transcript, temporary_root / "preview-run")

        if update_golden:
            GOLDEN_PATH.parent.mkdir(parents=True, exist_ok=True)
            GOLDEN_PATH.write_text(canonical_json(preview_result), encoding="utf-8")
            print(f"updated golden: {GOLDEN_PATH}")
        elif not GOLDEN_PATH.is_file():
            raise ReplayFailure(
                f"golden file is missing: {GOLDEN_PATH}; use --update-golden intentionally"
            )

        expected = load_json(GOLDEN_PATH)
        assert_equal("preview replay", preview_result, expected)
        print(
            "PASS Skill replay: "
            f"{len(LANGUAGES)} languages, {preview_result['analysis']['summary']['total_components']} "
            "components, fixed transcript, cleaned session"
        )

        if archive is None:
            return
        install_root = temporary_root / "installed-skill"
        installed_binary = extract_archive(archive, install_root)
        version = run_command(installed_binary, ["version"])
        if version.get("version") != "0.1.0":
            raise ReplayFailure(f"unexpected installed binary version: {version}")
        installed_result = execute_replay(
            installed_binary, transcript, temporary_root / "installed-run"
        )
        assert_equal("installed replay vs golden", installed_result, expected)
        assert_equal("preview vs installed replay", installed_result, preview_result)
        print(f"PASS ZIP install smoke: {archive} -> {installed_binary}")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, help="preview binary or its preview directory")
    parser.add_argument("--preview-dir", type=Path, help="preview directory containing scripts/")
    parser.add_argument("--archive", type=Path, help="Skill ZIP to extract and execute")
    parser.add_argument(
        "--update-golden",
        action="store_true",
        help="rewrite the checked-in golden from the preview run (intentional maintenance action)",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        binary_location = args.binary or args.preview_dir or ROOT / "preview"
        run(resolve_binary(binary_location), args.archive, args.update_golden)
    except ReplayFailure as exc:
        print(f"REPLAY FAILED: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
