"""Scheduling contracts: distinct heavy checks must not become per-push copies."""

from pathlib import Path
import re
import unittest


ROOT = Path(__file__).resolve().parents[2]
WORKFLOWS = ROOT / ".github" / "workflows"


class CadenceTests(unittest.TestCase):
    def test_miri_has_exactly_one_workflow_owner(self):
        owners = sorted(
            path.name for path in WORKFLOWS.glob("*.yml")
            if "run: bash scripts/test-miri.sh" in path.read_text(encoding="utf-8")
        )
        self.assertEqual(owners, ["miri.yml"])

    def test_deep_analysis_is_weekly_or_manual(self):
        for name in ("miri.yml", "codeql.yml"):
            with self.subTest(workflow=name):
                text = (WORKFLOWS / name).read_text(encoding="utf-8")
                self.assertIn("  workflow_dispatch:", text)
                self.assertIn("  schedule:", text)
                self.assertNotIn("  push:", text)
                self.assertNotIn("  pull_request:", text)

    def test_empty_packaging_hook_is_not_automatic(self):
        text = (WORKFLOWS / "build-package.yml").read_text(encoding="utf-8")
        self.assertIn("  workflow_dispatch:", text)
        self.assertNotIn("  push:", text)
        self.assertNotIn("  pull_request:", text)
        self.assertNotIn("  schedule:", text)

    def test_ready_transition_runs_deferred_build_profiles(self):
        text = (WORKFLOWS / "ci.yml").read_text(encoding="utf-8")
        self.assertIn("ready_for_review", text)
        for name in ("msrv", "release-build"):
            with self.subTest(job=name):
                match = re.search(
                    rf"(?ms)^  {re.escape(name)}:\n.*?(?=^  [\w-]+:\n|\Z)", text
                )
                self.assertIsNotNone(match)
                block = match.group()
                self.assertIn("needs.plan.outputs.rust == 'true'", block)
                self.assertIn("github.event_name != 'pull_request'", block)
                self.assertIn("github.event.pull_request.draft == false", block)

    def test_regular_ci_does_not_reintroduce_miri(self):
        text = (WORKFLOWS / "ci.yml").read_text(encoding="utf-8")
        self.assertNotIn("  miri:", text)
        self.assertNotIn("scripts/test-miri.sh", text)


if __name__ == "__main__":
    unittest.main()
