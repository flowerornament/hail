#!/usr/bin/env python3
"""Unit tests for scripts/release.py (pure text functions, no git or nix)."""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

import release  # noqa: E402


INTRO = "# Changelog\n\nAll notable changes to `hail` are documented in this file.\n\n"


class ScriptVersionTests(unittest.TestCase):
    def test_reads_the_single_version_line(self) -> None:
        text = '#!/usr/bin/env bash\nset -euo pipefail\n\nVERSION="0.2.5"\n\nKINDS="a b"\n'
        self.assertEqual(release.script_version_text(text), "0.2.5")

    def test_rejects_missing_or_duplicate_lines(self) -> None:
        with self.assertRaisesRegex(ValueError, "exactly one"):
            release.script_version_text('echo "hail $VERSION"\n')
        with self.assertRaisesRegex(ValueError, "exactly one"):
            release.script_version_text('VERSION="1.0.0"\nVERSION="1.0.1"\n')

    def test_set_rewrites_only_the_version_line(self) -> None:
        text = 'VERSION="0.2.5"\necho "hail doctor v${VERSION}"\n'
        self.assertEqual(
            release.set_script_version_text(text, "0.3.0"),
            'VERSION="0.3.0"\necho "hail doctor v${VERSION}"\n',
        )

    def test_the_real_script_declares_a_version(self) -> None:
        version = release.script_version_text(release.SCRIPT.read_text(encoding="utf-8"))
        self.assertRegex(version, r"^\d+\.\d+\.\d+$")


class ChangelogTests(unittest.TestCase):
    def test_release_notes_are_exactly_one_version_section(self) -> None:
        text = (
            INTRO
            + "## Unreleased\n\n- Future work.\n\n"
            + "## v0.2.0 - 2026-02-01\n\n- A note with a\n  wrapped continuation.\n- A second note.\n\n"
            + "## v0.1.0 - 2026-01-01\n\n- First release.\n"
        )
        notes = release.changelog_release_notes_text(text, "0.2.0")
        self.assertEqual(notes, "- A note with a\n  wrapped continuation.\n- A second note.\n")

    def test_release_notes_reject_missing_or_empty_sections(self) -> None:
        with self.assertRaisesRegex(ValueError, "missing an entry"):
            release.changelog_release_notes_text(INTRO, "0.2.0")
        with self.assertRaisesRegex(ValueError, "no release notes"):
            release.changelog_release_notes_text(INTRO + "## v0.2.0 - 2026-02-01\n\n", "0.2.0")

    def test_scaffold_goes_under_unreleased_and_reports_pending(self) -> None:
        text = (
            INTRO
            + "## Unreleased\n\n- Pending one.\n- Pending two,\n  wrapped.\n\n"
            + "## v0.1.0 - 2026-01-01\n\n- First release.\n"
        )
        updated, pending = release.changelog_insert_entry_text(text, "0.2.0", "2026-02-01")
        self.assertEqual(pending, ["Pending one.", "Pending two, wrapped."])
        self.assertEqual(
            updated,
            INTRO
            + "## Unreleased\n\n- Pending one.\n- Pending two,\n  wrapped.\n\n"
            + "## v0.2.0 - 2026-02-01\n\n- TODO: summarize release changes.\n\n"
            + "## v0.1.0 - 2026-01-01\n\n- First release.\n",
        )

    def test_scaffold_is_idempotent(self) -> None:
        text = INTRO + "## Unreleased\n\n## v0.2.0 - 2026-02-01\n\n- Done.\n"
        updated, pending = release.changelog_insert_entry_text(text, "0.2.0", "2026-02-02")
        self.assertEqual(updated, text)
        self.assertEqual(pending, [])

    def test_scaffold_requires_one_unreleased_section(self) -> None:
        with self.assertRaisesRegex(ValueError, "exactly one"):
            release.changelog_insert_entry_text(INTRO + "## v0.1.0 - 2026-01-01\n\n- x\n", "0.2.0", "2026-02-01")

    def test_unreleased_warning(self) -> None:
        self.assertIsNone(release.unreleased_warning("0.2.0", []))
        self.assertIn("2 entries", release.unreleased_warning("0.2.0", ["a", "b"]) or "")

    def test_the_real_changelog_has_the_declared_version(self) -> None:
        version = release.script_version_text(release.SCRIPT.read_text(encoding="utf-8"))
        text = release.CHANGELOG.read_text(encoding="utf-8")
        self.assertTrue(release.changelog_text_has_entry(text, version), f"CHANGELOG.md has no entry for {version}")


if __name__ == "__main__":
    unittest.main()
