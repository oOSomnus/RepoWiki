#!/usr/bin/env python3
"""Validate the minimal runtime RepoWiki Skill directory or ZIP archive."""

from __future__ import annotations

import argparse
import stat
import sys
import zipfile
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import NoReturn


COMMON_FILES = {
    "SKILL.md",
    "agents/openai.yaml",
    "references/cli-contract.md",
    "references/prompt-map.md",
}
FEW_SHOT_FILES = {
    "references/few-shots/README.md",
    "references/few-shots/clickhouse-overview.md",
    "references/few-shots/clickhouse-storage-engine.md",
    "references/few-shots/clickhouse-query-pipeline.md",
    "references/few-shots/clickhouse-ast-create-query.md",
}

REPO_WIKI_FILES = COMMON_FILES | FEW_SHOT_FILES
CHANGE_WIKI_FILES = REPO_WIKI_FILES | {"references/change-workflow.md"}
FILES_BY_PROFILE = {
    "repo-wiki": REPO_WIKI_FILES,
    "change-wiki": CHANGE_WIKI_FILES,
}
EXECUTABLE_FILES = {"scripts/codewiki", "scripts/codewiki.exe"}
ALLOWED_TOP_LEVEL = {"SKILL.md", "agents", "references", "scripts"}
FORBIDDEN_TOP_LEVEL = {
    ".git",
    ".codewiki",
    "target",
    "reference",
    "tests",
    "packaging",
    "dist",
    "src",
    "prompts",
    "engine",
    "Cargo.toml",
    "Cargo.lock",
    "README.md",
    "PACKAGE-MANIFEST.txt",
    "__MACOSX",
}


@dataclass
class LoadedPackage:
    files: dict[str, bytes]
    executable_files: set[str]


def fail(message: str) -> NoReturn:
    print(f"invalid Skill package: {message}", file=sys.stderr)
    raise SystemExit(1)


def normalized_name(raw_name: str) -> str:
    raw_name = raw_name.replace("\\", "/").rstrip("/")
    path = PurePosixPath(raw_name)
    if raw_name.startswith("/") or not path.parts:
        fail(f"unsafe package path: {raw_name}")
    if any(part in {"", ".", ".."} for part in path.parts):
        fail(f"unsafe package path: {raw_name}")
    if path.parts[0] in FORBIDDEN_TOP_LEVEL:
        fail(f"forbidden entry: {raw_name}")
    if path.parts[0] not in ALLOWED_TOP_LEVEL:
        fail(f"package is not flat or has an unsupported root: {raw_name}")
    return str(path)


def load_directory(location: Path) -> LoadedPackage:
    if not location.is_dir():
        fail(f"package directory does not exist: {location}")

    files: dict[str, bytes] = {}
    executable_files: set[str] = set()
    for entry in location.rglob("*"):
        relative = entry.relative_to(location).as_posix()
        name = normalized_name(relative)
        if entry.is_symlink():
            fail(f"symlinks are not supported: {relative}")
        if entry.is_dir():
            continue
        if name in files:
            fail(f"duplicate package path: {name}")
        files[name] = entry.read_bytes()
        if entry.stat().st_mode & stat.S_IXUSR:
            executable_files.add(name)
    return LoadedPackage(files, executable_files)


def load_zip(location: Path) -> LoadedPackage:
    try:
        package = zipfile.ZipFile(location)
    except (OSError, zipfile.BadZipFile) as exc:
        fail(str(exc))

    files: dict[str, bytes] = {}
    executable_files: set[str] = set()
    with package:
        for info in package.infolist():
            name = normalized_name(info.filename)
            if info.is_dir() or info.filename.endswith(("/", "\\")):
                continue
            mode = (info.external_attr >> 16) & 0xFFFF
            if stat.S_ISLNK(mode):
                fail(f"symlinks are not supported: {info.filename}")
            if name in files:
                fail(f"duplicate package path: {name}")
            files[name] = package.read(info)
            if mode & stat.S_IXUSR:
                executable_files.add(name)
    return LoadedPackage(files, executable_files)


def load_package(location: str) -> LoadedPackage:
    path = Path(location)
    if path.is_dir():
        return load_directory(path)
    return load_zip(path)


def validate_package(package: LoadedPackage, label: str, profile: str) -> None:
    files = set(package.files)
    required = FILES_BY_PROFILE[profile]
    missing = sorted(required - files)
    if missing:
        fail(f"{label} is missing required files: {', '.join(missing)}")

    binaries = sorted(files & EXECUTABLE_FILES)
    if binaries != ["scripts/codewiki"] and binaries != ["scripts/codewiki.exe"]:
        fail(
            f"{label} must contain exactly one platform binary under scripts/: "
            f"found {binaries}"
        )

    expected = required | set(binaries)
    extra = sorted(files - expected)
    if extra:
        fail(f"{label} contains non-runtime files: {', '.join(extra)}")

    if binaries == ["scripts/codewiki"] and "scripts/codewiki" not in package.executable_files:
        fail(f"{label} POSIX binary is not executable")

    skill = package.files["SKILL.md"].decode("utf-8")
    frontmatter = skill.split("---", 2)
    expected_name = f"name: {profile}"
    if (
        not skill.startswith("---\n")
        or len(frontmatter) != 3
        or expected_name not in frontmatter[1].splitlines()
    ):
        fail(f"{label} has invalid or missing {profile} SKILL.md frontmatter")
    if "scripts/codewiki" not in skill:
        fail(f"{label} SKILL.md does not describe the bundled executable")
    if profile == "change-wiki":
        agent = package.files["agents/openai.yaml"].decode("utf-8")
        if not any(
            line.strip() == "allow_implicit_invocation: false"
            for line in agent.splitlines()
        ):
            fail(f"{label} change-wiki platform metadata must disable implicit invocation")


def compare_packages(left: LoadedPackage, right: LoadedPackage) -> None:
    if left.files.keys() != right.files.keys():
        only_left = sorted(left.files.keys() - right.files.keys())
        only_right = sorted(right.files.keys() - left.files.keys())
        fail(f"preview and ZIP file sets differ: only_preview={only_left}, only_zip={only_right}")
    different = sorted(name for name in left.files if left.files[name] != right.files[name])
    if different:
        fail(f"preview and ZIP contents differ: {', '.join(different)}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--profile",
        choices=tuple(FILES_BY_PROFILE),
        default="repo-wiki",
        help="runtime Skill package profile (default: repo-wiki)",
    )
    parser.add_argument("packages", nargs="+", metavar="PACKAGE_DIR_OR_ZIP")
    args = parser.parse_args()
    if len(args.packages) not in {1, 2}:
        parser.error("provide one package or a preview and ZIP pair")

    first = load_package(args.packages[0])
    validate_package(first, args.packages[0], args.profile)
    if len(args.packages) == 2:
        second = load_package(args.packages[1])
        validate_package(second, args.packages[1], args.profile)
        compare_packages(first, second)
        print(
            f"valid matching {args.profile} Skill packages: "
            f"{args.packages[0]} and {args.packages[1]}"
        )
    else:
        print(f"valid {args.profile} Skill package: {args.packages[0]}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
