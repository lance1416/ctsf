#!/usr/bin/env python3
"""Independent CLI/hook/CI context checks; all Git mutations use temporary repos."""

import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

from run_git_policy import PROJECT, ZERO_SHA, git, isolated_env, make_history


SCRIPT = PROJECT / "scripts/git_policy.py"


class PolicyContextTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="ctsf-setup-context-")
        self.addCleanup(self.directory.cleanup)
        self.repo = Path(self.directory.name)
        self.env = isolated_env()

    def invoke(self, *args, env=None):
        return subprocess.run(
            [sys.executable, str(SCRIPT), *args], cwd=self.repo,
            env=self.env if env is None else env,
            capture_output=True, text=True, timeout=15,
        )

    def accepted(self, result):
        self.assertEqual(result.returncode, 0, result.stderr)

    def rejected(self, result):
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertNotIn("Traceback (most recent call last)", result.stderr)

    def history(self, shape="linear", messages=None):
        return make_history(self.repo, self.env, {
            "shape": shape,
            "messages": messages or ["chore: root\n", "feat: head\n"],
        })

    def event(self, name, payload):
        path = self.repo / "event.json"
        path.write_text(json.dumps(payload), encoding="utf-8")
        env = dict(self.env, GITHUB_EVENT_NAME=name, GITHUB_EVENT_PATH=str(path))
        return self.invoke("ci", env=env)

    def pr(self, history, *, branch="feat/setup-checks", title="test: setup policy"):
        return {
            "pull_request": {
                "head": {"ref": branch, "sha": history["head"]},
                "base": {"sha": history["base"]}, "title": title,
            }
        }

    def test_unborn_history_is_explicitly_not_run(self):
        git(self.repo, self.env, "init", "--initial-branch=main", "--template=", ".")
        result = self.invoke("local")
        self.accepted(result)
        self.assertIn("UNBORN", result.stdout)
        self.assertIn("NOT RUN", result.stdout)
        self.assertNotIn("checked 1", result.stdout)

    def test_empty_range_reports_no_messages_checked(self):
        history = self.history("empty", ["chore: root\n"])
        result = self.invoke("history", history["head"], "--base", history["base"])
        self.accepted(result)
        self.assertIn("empty range", result.stdout)
        self.assertIn("no commit messages were checked", result.stdout)

    def test_shallow_history_fails_closed(self):
        history = self.history()
        (self.repo / ".git/shallow").write_text(history["base"] + "\n")
        result = self.invoke("history", history["head"], "--base", history["base"])
        self.rejected(result)
        self.assertIn("full clone", result.stderr)

    def test_missing_base_is_not_an_empty_success(self):
        history = self.history()
        self.rejected(self.invoke("history", history["head"], "--base", "f" * 40))

    def test_initial_push_checks_root(self):
        history = self.history("root", ["chore: root\n"])
        result = self.event("push", {
            "ref": "refs/heads/main", "before": ZERO_SHA, "after": history["head"],
        })
        self.accepted(result)
        self.assertIn("checked 1 complete commit", result.stdout)

    def test_initial_push_rejects_invalid_root(self):
        history = self.history("root", ["chore: root\n\nCoAuthor: Example\n"])
        self.rejected(self.event("push", {
            "ref": "refs/heads/main", "before": ZERO_SHA, "after": history["head"],
        }))

    def test_push_checks_feature_branch(self):
        history = self.history()
        for branch, accept in (("feat/setup-checks", True), ("feat/Setup_Checks", False)):
            with self.subTest(branch=branch):
                result = self.event("push", {
                    "ref": "refs/heads/" + branch,
                    "before": history["base"], "after": history["head"],
                })
                (self.accepted if accept else self.rejected)(result)

    def test_deleted_push_reports_no_checks(self):
        result = self.event("push", {"deleted": True})
        self.accepted(result)
        self.assertIn("no branch head or commits to check", result.stdout)

    def test_pr_uses_actual_head_not_detached_synthetic_merge(self):
        history = self.history("merge", [
            "chore: root\n", "feat: head\n", "fix: base-side\n", "synthetic merge\n",
        ])
        parents = git(self.repo, self.env, "rev-list", "--parents", "-n", "1", history["head"]).split()
        git(self.repo, self.env, "update-ref", "--no-deref", "HEAD", history["head"])
        result = self.event("pull_request", self.pr({"base": history["base"], "head": parents[1]}))
        self.accepted(result)
        self.assertIn("checked 1 complete commit", result.stdout)

    def test_pr_rejects_actual_feature_merge(self):
        history = self.history("merge", [
            "chore: root\n", "feat: left\n", "fix: right\n", "chore: combine\n",
        ])
        result = self.event("pull_request", self.pr(history))
        self.rejected(result)
        self.assertIn("merge commit forbidden", result.stderr)

    def test_pr_title_and_feature_branch_are_validated(self):
        history = self.history()
        for branch, title in (
            ("feat/Setup_Checks", "test: valid title"),
            ("feat/setup-checks", "invalid title"),
            ("feat/setup-checks", "test: title\n\nBody"),
            ("feat/setup-checks", "test: title\nCoAuthor: Example"),
        ):
            with self.subTest(branch=branch, title=title):
                self.rejected(self.event("pull_request", self.pr(history, branch=branch, title=title)))

    def test_pr_checks_earlier_complete_messages(self):
        history = self.history(messages=[
            "chore: root\n", "feat: change\n\nBody\n", "fix: valid head\n",
        ])
        self.rejected(self.event("pull_request", self.pr(history)))

    def test_commit_msg_hook_without_installing_config(self):
        git(self.repo, self.env, "init", "--initial-branch=main", "--template=", ".")
        (self.repo / "scripts").mkdir()
        (self.repo / ".githooks").mkdir()
        shutil.copy2(SCRIPT, self.repo / "scripts/git_policy.py")
        hook = self.repo / ".githooks/commit-msg"
        shutil.copy2(PROJECT / ".githooks/commit-msg", hook)
        for value, accept in (
            (b"chore: setup\n", True), (b"feat(kernel)!: update contract\n", True),
            (b"chore: setup\n\nBody\n", False),
            (b"chore: setup\nCo-authored-by: Example\n", False),
        ):
            with self.subTest(value=value):
                message = self.repo / "COMMIT_EDITMSG"
                message.write_bytes(value)
                result = subprocess.run(
                    [str(hook), str(message)], cwd=self.repo, env=self.env,
                    capture_output=True, text=True, timeout=15,
                )
                (self.accepted if accept else self.rejected)(result)


if __name__ == "__main__":
    unittest.main(verbosity=2)
