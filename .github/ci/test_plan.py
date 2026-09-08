"""Cheap CI-policy regressions; no Rust build or network is required."""

from pathlib import Path
from types import SimpleNamespace
import subprocess
import unittest
from unittest.mock import patch

from plan import CHECKS, changed_paths, classify, covered_by_pr, discover_duplicate


class SelectionTests(unittest.TestCase):
    def test_documentation_does_not_build_rust(self):
        checks = classify(["README.md", "docs/guia-rapido.pt_BR.md"])
        self.assertFalse(checks["native"])
        self.assertFalse(checks["rust"])
        self.assertFalse(checks["dependencies"])
        self.assertTrue(checks["secrets"])

    def test_catalogs_only_select_translation_and_text_checks(self):
        checks = classify(["po/pt_BR.po", "po/biglinux-microphone.pot"])
        self.assertEqual({key for key, value in checks.items() if value}, {"i18n", "spelling", "secrets"})

    def test_rust_edits_do_not_repeat_dependency_audits(self):
        checks = classify(["src/services/reconcile.rs"])
        self.assertTrue(checks["native"])
        self.assertTrue(checks["rust"])
        self.assertTrue(checks["i18n"])
        self.assertFalse(checks["dependencies"])

    def test_both_lockfiles_select_audit(self):
        for path in ("Cargo.lock", "vendor/big-rust-components/Cargo.lock"):
            with self.subTest(path=path):
                self.assertTrue(classify([path])["dependencies"])

    def test_ci_control_changes_cannot_skip_checks(self):
        for path in (".github/workflows/ci.yml", ".github/ci/plan.py", ".github/actions/install-system-deps/action.yml"):
            with self.subTest(path=path):
                self.assertTrue(all(classify([path]).values()))

    def test_qml_selects_translation_and_plasma_contracts(self):
        checks = classify(["usr/share/plasma/plasmoids/example/contents/ui/main.qml"])
        self.assertTrue(checks["plasma"])
        self.assertTrue(checks["i18n"])
        self.assertFalse(checks["rust"])

    def test_ui_script_selects_native_without_release_rebuild(self):
        checks = classify(["scripts/test-ui.sh"])
        self.assertTrue(checks["shell"])
        self.assertTrue(checks["native"])
        self.assertFalse(checks["rust"])

    def test_unknown_inputs_and_missing_history_run_everything(self):
        self.assertTrue(all(classify(["new-build-input.xyz"]).values()))
        self.assertTrue(all(classify(None).values()))
        self.assertFalse(any(classify([]).values()))

    def test_large_diff_has_no_api_300_file_limit(self):
        paths = [f"docs/note-{number}.md" for number in range(350)] + ["src/lib.rs"]
        self.assertTrue(classify(paths)["native"])

    def test_git_diff_is_nul_delimited_and_disables_rename_hiding(self):
        with patch("plan.subprocess.run", return_value=SimpleNamespace(stdout=b"docs/a\nb.md\0src/lib.rs\0")) as run:
            paths = changed_paths("push", {"before": "a" * 40})
        self.assertEqual(paths, ["docs/a\nb.md", "src/lib.rs"])
        self.assertIn("--no-renames", run.call_args.args[0])
        self.assertEqual(run.call_args.args[0][-1], "--")

    def test_new_branches_and_invalid_revisions_run_everything(self):
        for base in ("0" * 40, "--output=/tmp/unwanted", ""):
            with self.subTest(base=base):
                self.assertIsNone(changed_paths("push", {"before": base}))
        self.assertIsNone(changed_paths("workflow_dispatch", {}))

    def test_missing_base_object_runs_everything(self):
        with patch("plan.subprocess.run", side_effect=subprocess.CalledProcessError(128, "git")):
            self.assertIsNone(changed_paths("push", {"before": "a" * 40}))


class DuplicateTests(unittest.TestCase):
    def setUp(self):
        self.repo = "biglinux/biglinux-noise-reduction-pipewire"
        self.event = {"ref": "refs/heads/testing-audio", "after": "b" * 40}
        self.pr = {"state": "open", "base": {"ref": "main"}, "head": {
            "ref": "testing-audio", "sha": "b" * 40, "repo": {"full_name": self.repo},
        }}

    def test_exact_same_repository_branch_and_revision_is_covered(self):
        self.assertTrue(covered_by_pr(self.event, self.repo, [self.pr]))

    def test_main_push_is_never_suppressed(self):
        self.event["ref"] = "refs/heads/main"
        self.assertFalse(covered_by_pr(self.event, self.repo, [self.pr]))

    def test_closed_or_old_or_fork_pr_cannot_suppress_push(self):
        import copy
        for field in ("state", "sha", "repo", "base"):
            pr = copy.deepcopy(self.pr)
            if field == "state":
                pr["state"] = "closed"
            elif field == "sha":
                pr["head"]["sha"] = "c" * 40
            elif field == "repo":
                pr["head"]["repo"]["full_name"] = "another/fork"
            else:
                pr["base"]["ref"] = "development"
            with self.subTest(field=field):
                self.assertFalse(covered_by_pr(self.event, self.repo, [pr]))

    def test_api_failure_keeps_push_checks(self):
        with patch.dict("os.environ", {"GITHUB_REPOSITORY": self.repo, "GH_TOKEN": "test-only"}), patch("plan.urlopen", side_effect=OSError("offline")):
            self.assertFalse(discover_duplicate(self.event))


class WorkflowContracts(unittest.TestCase):
    def test_each_application_suite_has_one_owner(self):
        root = Path(__file__).resolve().parents[2]
        ci = (root / ".github/workflows/ci.yml").read_text()
        self.assertEqual(ci.count("run: cargo test "), 1)
        self.assertEqual(ci.count("run: bash scripts/test-ui.sh "), 1)
        self.assertEqual(ci.count("run: bash scripts/test-pipewire.sh "), 1)
        self.assertNotIn("scripts/ci-jemalloc.sh", ci)
        self.assertNotIn("dependency-review-action", ci)
        self.assertIn("cargo deny check bans licenses sources", ci)
        self.assertIn("ci-${{ github.event_name }}-", ci)
        for check in CHECKS:
            self.assertIn(f"steps.plan.outputs.{check}", ci)

    def test_scheduled_security_does_not_repeat_application_tests(self):
        root = Path(__file__).resolve().parents[2]
        security = (root / ".github/workflows/security.yml").read_text()
        self.assertNotIn("  push:", security)
        self.assertNotIn("  pull_request:", security)
        self.assertNotIn("miri", security)
        self.assertNotIn("cargo test", security)
        self.assertIn("schedule:", security)


if __name__ == "__main__":
    unittest.main()
