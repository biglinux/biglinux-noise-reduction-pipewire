"""Release acceptance must not confuse deliberate deferral with a passed build."""

import unittest
from pathlib import Path

from plan import CHECKS, classify
from result import RELEASE_JOBS, SELECTORS, blocked_jobs


class ResultTests(unittest.TestCase):
    def results(self, enabled=True):
        jobs = {name: {"result": "success" if enabled else "skipped"}
                for name in (*SELECTORS, *RELEASE_JOBS)}
        jobs["plan"] = {"result": "success", "outputs": dict.fromkeys(CHECKS, str(enabled).lower())}
        return jobs

    def test_complete_release_is_accepted(self):
        self.assertEqual(blocked_jobs(self.results(), True), {})

    def test_selected_job_cannot_be_skipped(self):
        for name in SELECTORS:
            with self.subTest(job=name):
                jobs = self.results()
                jobs[name]["result"] = "skipped"
                self.assertEqual(blocked_jobs(jobs, True), {name: "skipped"})

    def test_candidate_requires_both_release_profiles(self):
        for name in RELEASE_JOBS:
            with self.subTest(job=name):
                jobs = self.results()
                jobs[name]["result"] = "skipped"
                self.assertEqual(blocked_jobs(jobs, True), {name: "skipped"})
                self.assertEqual(blocked_jobs(jobs, False), {})

    def test_non_applicable_jobs_remain_skipped(self):
        self.assertEqual(blocked_jobs(self.results(False), True), {})

    def test_failed_or_missing_planner_blocks_acceptance(self):
        for status in ("failure", "cancelled", "skipped"):
            jobs = self.results()
            jobs["plan"]["result"] = status
            self.assertIn("plan", blocked_jobs(jobs, True))
        jobs = self.results()
        del jobs["plan"]["outputs"]["native"]
        self.assertIn("plan", blocked_jobs(jobs, True))
        self.assertIn("plan", blocked_jobs({}, True))

    def test_failures_are_never_hidden_by_scope(self):
        for status in ("failure", "cancelled", "missing"):
            jobs = self.results(False)
            jobs["native"]["result"] = status
            self.assertEqual(blocked_jobs(jobs, False), {"native": status})

    def test_package_inputs_select_builds(self):
        for path in ("packaging/arch/PKGBUILD", "scripts/test-package.sh", "scripts/verify-package.py"):
            with self.subTest(path=path):
                checks = classify([path])
                self.assertTrue(checks["rust"])
                self.assertTrue(checks["shell"])

    def test_release_gate_preserves_single_suite_ownership(self):
        root = Path(__file__).resolve().parents[2]
        ci = (root / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        self.assertEqual(ci.count("run: bash scripts/test-package.sh "), 1)
        self.assertEqual(ci.count("run: cargo test "), 1)
        self.assertEqual(ci.count("run: bash scripts/test-ui.sh "), 1)
        self.assertEqual(ci.count("run: bash scripts/test-pipewire.sh "), 1)
        self.assertIn("release-candidate", ci)
        self.assertIn("python3 .github/ci/result.py", ci)


if __name__ == "__main__":
    unittest.main()
