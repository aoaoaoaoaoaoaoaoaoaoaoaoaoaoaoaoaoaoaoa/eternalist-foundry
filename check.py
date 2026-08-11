#!/usr/bin/env python3
from __future__ import annotations

import argparse
import os
import subprocess
import tomllib
from dataclasses import dataclass
from pathlib import Path, PurePosixPath


ROOT = Path(__file__).resolve().parent
WORKSPACE_MANIFEST = ROOT / "Cargo.toml"
IGNORED_SOURCE_DIRS = frozenset(
    {".direnv", ".git", ".hg", ".jj", ".svn", "__pycache__", "node_modules", "target", "vendor"}
)
Command = tuple[str, ...]
CommandSequence = tuple[Command, ...]


@dataclass(frozen=True, slots=True)
class SourceFilePolicy:
    max_lines: int
    include: tuple[str, ...]
    exclude: tuple[str, ...]


def load_metadata() -> dict[str, object]:
    workspace = tomllib.loads(WORKSPACE_MANIFEST.read_text(encoding="utf-8"))
    return workspace["workspace"]["metadata"]["rust-starter"]


def load_command(value: object, key: str) -> Command:
    if not isinstance(value, list) or not value or not all(isinstance(part, str) and part for part in value):
        raise SystemExit(f"[check] invalid {key}: expected a non-empty string list")
    return tuple(value)


def load_commands(metadata: dict[str, object]) -> dict[str, Command | CommandSequence]:
    sequence = metadata.get("canonicalize_commands")
    if not isinstance(sequence, list) or not sequence:
        raise SystemExit("[check] invalid canonicalize_commands")
    canonicalize = tuple(load_command(command, "canonicalize_commands") for command in sequence)
    return {
        "format": load_command(metadata.get("format_command"), "format_command"),
        "clippy": load_command(metadata.get("clippy_command"), "clippy_command"),
        "test": load_command(metadata.get("test_command"), "test_command"),
        "doc": load_command(metadata.get("doc_command"), "doc_command"),
        "canonicalize": canonicalize,
    }


def load_policy(metadata: dict[str, object]) -> SourceFilePolicy:
    raw = metadata["source_files"]
    if not isinstance(raw, dict):
        raise SystemExit("[check] invalid source_files")
    max_lines = raw.get("max_lines")
    include = raw.get("include")
    exclude = raw.get("exclude")
    if not isinstance(max_lines, int) or max_lines <= 0:
        raise SystemExit("[check] max_lines must be positive")
    if not isinstance(include, list) or not all(isinstance(value, str) and value for value in include):
        raise SystemExit("[check] include must contain patterns")
    if not isinstance(exclude, list) or not all(isinstance(value, str) and value for value in exclude):
        raise SystemExit("[check] exclude must contain patterns")
    return SourceFilePolicy(max_lines, tuple(include), tuple(exclude))


def matches(path: PurePosixPath, pattern: str) -> bool:
    return path.match(pattern) or (pattern.startswith("**/") and path.match(pattern[3:]))


def enforce(policy: SourceFilePolicy) -> None:
    violations: list[tuple[str, int]] = []
    for current_root, dirnames, filenames in os.walk(ROOT):
        dirnames[:] = sorted(name for name in dirnames if name not in IGNORED_SOURCE_DIRS)
        for filename in filenames:
            path = Path(current_root) / filename
            relative = PurePosixPath(path.relative_to(ROOT).as_posix())
            if not any(matches(relative, pattern) for pattern in policy.include):
                continue
            if any(matches(relative, pattern) for pattern in policy.exclude):
                continue
            lines = len(path.read_text(encoding="utf-8").splitlines())
            if lines > policy.max_lines:
                violations.append((relative.as_posix(), lines))
    for path, lines in violations:
        print(f"[check] {path}: {lines} lines exceeds {policy.max_lines}", flush=True)
    if violations:
        raise SystemExit(1)


def run(name: str, command: Command) -> None:
    print(f"[check] {name}: {' '.join(command)}", flush=True)
    result = subprocess.run(command, cwd=ROOT)
    if result.returncode != 0:
        raise SystemExit(result.returncode)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=("check", "verify", "deep", "fix", "canon"), nargs="?", default="check")
    mode = parser.parse_args().mode
    metadata = load_metadata()
    commands = load_commands(metadata)
    if mode not in {"fix", "canon"}:
        enforce(load_policy(metadata))
    if mode != "verify":
        for index, command in enumerate(commands["canonicalize"], start=1):
            run(f"canonicalize.{index}", command)
    if mode in {"fix", "canon"}:
        return
    run("fmt", commands["format"])
    run("clippy", commands["clippy"])
    run("test", commands["test"])
    if mode == "deep":
        run("doc", commands["doc"])


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        raise SystemExit(130)
