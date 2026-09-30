#!/usr/bin/env python3
"""Release helper for hail.

The one version source is the VERSION= line in bin/hail. `bump` rewrites it
and scaffolds a CHANGELOG entry, `verify` proves the tree is releasable,
`tag` pushes an annotated tag and moves the `release` branch, and `notes`
renders one CHANGELOG section for the GitHub release.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from datetime import date
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
SCRIPT = ROOT / "bin" / "hail"
CHANGELOG = ROOT / "CHANGELOG.md"
SEMVER_RE = re.compile(r"^\d+\.\d+\.\d+$")
VERSION_LINE_RE = re.compile(r'(?m)^VERSION="(\d+\.\d+\.\d+)"$')
CHANGELOG_INTRO_MARKER = "All notable changes to `hail` are documented in this file.\n\n"
UNRELEASED_HEADING = "## Unreleased"
RELEASE_BRANCH = "release"


def fail(message: str) -> None:
    print(f"error: {message}", file=sys.stderr)
    raise SystemExit(1)


def read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def write_text(path: Path, text: str) -> None:
    path.write_text(text, encoding="utf-8")


# --- versions ---------------------------------------------------------------


def script_version_text(text: str) -> str:
    matches = VERSION_LINE_RE.findall(text)
    if len(matches) != 1:
        raise ValueError("bin/hail must contain exactly one VERSION=\"x.y.z\" line")
    return matches[0]


def script_version() -> str:
    try:
        return script_version_text(read_text(SCRIPT))
    except ValueError as error:
        fail(str(error))


def set_script_version_text(text: str, version: str) -> str:
    updated, count = VERSION_LINE_RE.subn(f'VERSION="{version}"', text, count=1)
    if count != 1:
        raise ValueError("bin/hail must contain exactly one VERSION=\"x.y.z\" line")
    return updated


def nix_version() -> str:
    system = capture(["nix", "eval", "--impure", "--raw", "--expr", "builtins.currentSystem"])
    return capture(["nix", "eval", "--raw", f".#packages.{system}.default.version"])


# --- changelog --------------------------------------------------------------


def changelog_text() -> str:
    return read_text(CHANGELOG)


def changelog_heading_re(version: str) -> str:
    return rf"(?m)^## v{re.escape(version)} - \d{{4}}-\d{{2}}-\d{{2}}$"


def changelog_text_has_entry(text: str, version: str) -> bool:
    return re.search(changelog_heading_re(version), text) is not None


def changelog_release_notes_text(text: str, version: str) -> str:
    heading = re.search(changelog_heading_re(version), text)
    if heading is None:
        raise ValueError(f"CHANGELOG.md is missing an entry for {version}")
    next_heading = re.search(r"(?m)^## ", text[heading.end() :])
    end = len(text) if next_heading is None else heading.end() + next_heading.start()
    notes = text[heading.end() : end].strip()
    if not notes or re.search(r"(?m)^- ", notes) is None:
        raise ValueError(f"CHANGELOG.md entry for {version} has no release notes")
    return f"{notes}\n"


def changelog_entry(version: str) -> str:
    try:
        return changelog_release_notes_text(changelog_text(), version)
    except ValueError as error:
        fail(str(error))


def changelog_entry_is_ready(version: str) -> bool:
    entry = changelog_entry(version)
    if "TODO:" in entry or "TBD" in entry:
        return False
    return True


def changelog_scaffold(version: str, today: str) -> str:
    return f"## v{version} - {today}\n\n- TODO: summarize release changes.\n\n"


def changelog_pending_entries(unreleased_block: str) -> list[str]:
    entries = []
    for bullet in re.finditer(r"(?m)^- ", unreleased_block):
        boundary = re.search(r"(?m)^- ", unreleased_block[bullet.end() :])
        end = len(unreleased_block) if boundary is None else bullet.end() + boundary.start()
        entries.append(" ".join(unreleased_block[bullet.end() : end].split()))
    return entries


def changelog_insert_entry_text(text: str, version: str, today: str) -> tuple[str, list[str]]:
    """Insert a scaffold for `version` directly under ## Unreleased.

    Returns the new text and the bullets still sitting under Unreleased, so the
    caller can warn that they may belong in the release being cut.
    """
    if CHANGELOG_INTRO_MARKER not in text:
        raise ValueError("could not find CHANGELOG.md insertion marker")
    unreleased_matches = list(re.finditer(rf"(?m)^{re.escape(UNRELEASED_HEADING)}\s*$", text))
    if len(unreleased_matches) != 1:
        raise ValueError("CHANGELOG.md must contain exactly one ## Unreleased section")
    unreleased = unreleased_matches[0]
    next_heading = re.search(r"(?m)^## ", text[unreleased.end() :])
    unreleased_end = len(text) if next_heading is None else unreleased.end() + next_heading.start()
    pending = changelog_pending_entries(text[unreleased.start() : unreleased_end])
    if changelog_text_has_entry(text, version):
        return text, pending
    updated = text[:unreleased_end] + changelog_scaffold(version, today) + text[unreleased_end:]
    return updated, pending


def unreleased_warning(version: str, pending: list[str]) -> str | None:
    if not pending:
        return None
    entries = "\n".join(f"  - {entry}" for entry in pending)
    return (
        f"warning: CHANGELOG.md Unreleased still contains {len(pending)} entries "
        f"after scaffolding v{version}:\n{entries}\n"
        f"Review whether they belong in v{version}."
    )


# --- commands ---------------------------------------------------------------


def bump(version: str) -> None:
    if SEMVER_RE.fullmatch(version) is None:
        fail("version must be semver like 0.3.1")
    try:
        write_text(SCRIPT, set_script_version_text(read_text(SCRIPT), version))
        updated, pending = changelog_insert_entry_text(
            changelog_text(), version, date.today().isoformat()
        )
    except ValueError as error:
        fail(str(error))
    write_text(CHANGELOG, updated)
    warning = unreleased_warning(version, pending)
    if warning is not None:
        print(warning, file=sys.stderr)
    print(f"updated release version to {version}")
    print("  - bin/hail")
    print("  - CHANGELOG.md")
    print("Fill in the CHANGELOG.md entry, commit, push, then `just release-tag`.")


def run(cmd: list[str]) -> None:
    print(f"+ {' '.join(cmd)}", flush=True)
    subprocess.run(cmd, cwd=ROOT, check=True)


def capture(cmd: list[str]) -> str:
    result = subprocess.run(cmd, cwd=ROOT, check=True, stdout=subprocess.PIPE, text=True)
    return result.stdout.strip()


def command_succeeds(cmd: list[str]) -> bool:
    return (
        subprocess.run(cmd, cwd=ROOT, check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode
        == 0
    )


def require_clean_worktree() -> None:
    status = capture(["git", "status", "--porcelain"])
    entries = [line for line in status.splitlines() if line.strip()]
    if not entries:
        return
    preview = "\n".join(f"  {entry}" for entry in entries[:10])
    suffix = "" if len(entries) <= 10 else f"\n  ... and {len(entries) - 10} more"
    fail(f"git working tree must be clean; commit first\n{preview}{suffix}")


def require_pushed(remote: str = "origin", branch: str = "main") -> None:
    run(["git", "fetch", "--quiet", remote, branch])
    if not command_succeeds(["git", "merge-base", "--is-ancestor", "HEAD", f"{remote}/{branch}"]):
        fail(f"HEAD is not on {remote}/{branch}; push before tagging")


def verify() -> None:
    version = script_version()
    nix = nix_version()
    if nix != version:
        fail(f"release versions do not match: bin/hail={version}, nix={nix}")
    if not changelog_entry_is_ready(version):
        fail(
            "CHANGELOG.md must contain a release entry for "
            f"{version} with at least one bullet and no TODO/TBD placeholders"
        )
    require_clean_worktree()
    run(["just", "check"])
    output = capture(["nix", "build", "--no-link", "--print-out-paths", "."])
    print(f"+ nix build -> {output}")
    reported = capture([f"{output}/bin/hail", "--version"])
    if reported != f"hail {version}":
        fail(f"built package reports {reported!r}, expected 'hail {version}'")
    print(f"release verification passed for {version}")


def tag(version: str) -> None:
    if SEMVER_RE.fullmatch(version) is None:
        fail("version must be semver like 0.3.1")
    current = script_version()
    if current != version:
        fail(f"bin/hail version is {current}, expected {version}")
    if not changelog_entry_is_ready(version):
        fail(f"CHANGELOG.md entry for {version} is not ready")
    require_clean_worktree()
    tag_name = f"v{version}"
    if capture(["git", "tag", "--list", tag_name]):
        fail(f"tag {tag_name} already exists")
    if capture(["git", "ls-remote", "--tags", "origin", f"refs/tags/{tag_name}"]):
        fail(f"tag {tag_name} already exists on origin")
    require_pushed()
    run(["git", "tag", "-a", tag_name, "-m", tag_name])
    run(["git", "push", "origin", tag_name])
    # Keep the moving release branch pointed at the latest published tag for
    # downstream release-tracking flake inputs (nix-config follows it).
    run(["git", "branch", "-f", RELEASE_BRANCH, tag_name])
    run(["git", "push", "--force-with-lease", "origin", f"refs/heads/{RELEASE_BRANCH}:refs/heads/{RELEASE_BRANCH}"])
    print(f"released {tag_name}; the GitHub release is published by .github/workflows/release.yml")


def print_release_notes(version_or_tag: str) -> None:
    version = version_or_tag.removeprefix("v")
    if SEMVER_RE.fullmatch(version) is None:
        fail("version must be semver like 0.3.1 or a tag like v0.3.1")
    print(changelog_entry(version), end="")


def main() -> None:
    parser = argparse.ArgumentParser(description="Release helper for hail")
    subparsers = parser.add_subparsers(dest="command", required=True)
    subparsers.add_parser("bump", help="set the version in bin/hail and scaffold CHANGELOG.md").add_argument("version")
    subparsers.add_parser("verify", help="run release readiness checks")
    subparsers.add_parser("tag", help="create and push a release tag, move the release branch").add_argument("version")
    subparsers.add_parser("notes", help="render one CHANGELOG section as GitHub release notes").add_argument("version")
    subparsers.add_parser("version", help="print the version declared in bin/hail")
    args = parser.parse_args()
    if args.command == "bump":
        bump(args.version)
    elif args.command == "verify":
        verify()
    elif args.command == "tag":
        tag(args.version)
    elif args.command == "notes":
        print_release_notes(args.version)
    else:
        print(script_version())


if __name__ == "__main__":
    main()
