#!/usr/bin/env python3
"""Acceptance checks use pure inputs and disposable repositories only."""

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from git_policy import message_error, require_branch

POLICY = str(Path(__file__).with_name("git_policy.py").resolve())


class MessageTests(unittest.TestCase):
    def test_valid_subjects(self):
        for message in [b"feat: add health", b"fix(core)!: repair API\n",
                        "docs: 更新说明\n".encode(), b"chore(repo): setup\r\n"]:
            with self.subTest(message=message):
                self.assertIsNone(message_error(message))

    def test_rejected_messages(self):
        for message in [b"", b"Merge branch 'x'", b"unknown: subject", b"feat: ",
                        b"feat: subject \n", b"feat: subject\n\n",
                        b"feat: subject\nbody", b"feat: subject\n\nCo-authored-by: X",
                        b"feat: CoAuthor: X", b"feat: Co-authored-by: X", b"\xff",
                        "feat: subject\u2028body".encode()]:
            with self.subTest(message=message):
                self.assertIsNotNone(message_error(message))

    def test_branch_names(self):
        for name in ["main", "feat/task-editor", "chore/project-setup", "fix/race-2"]:
            require_branch(name)
        for name in ["codex/setup", "feature/x", "feat/Upper", "feat/a--b", "feat/a/b", ""]:
            with self.subTest(name=name), self.assertRaises(ValueError):
                require_branch(name)


class HistoryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="ctsf-git-policy-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        self.env.update(GIT_AUTHOR_NAME="Policy Test", GIT_AUTHOR_EMAIL="test@example.invalid",
                        GIT_COMMITTER_NAME="Policy Test", GIT_COMMITTER_EMAIL="test@example.invalid",
                        GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull,
                        GIT_CONFIG_SYSTEM=os.devnull)
        self.git("init", "--initial-branch=main", "--template=", str(self.root))
        self.tree = self.git("mktree", data=b"").strip().decode()

    def git(self, *args, data=None):
        return subprocess.run(["git", *args], cwd=self.root, env=self.env, input=data,
                              check=True, capture_output=True).stdout

    def commit(self, message, *parents):
        args = ["commit-tree", self.tree]
        for parent in parents:
            args.extend(["-p", parent])
        return self.git(*args, data=message).strip().decode()

    def policy(self, *args, env=None):
        return subprocess.run([sys.executable, POLICY, *args], cwd=self.root,
                              env=env or self.env, capture_output=True, text=True)

    def test_unborn_and_first_commit(self):
        result = self.policy("local")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("NOT RUN", result.stdout)
        first = self.commit(b"chore: initial setup\n")
        self.assertEqual(self.policy("history", first).returncode, 0)

    def test_complete_messages_and_merge_commits(self):
        first = self.commit(b"chore: initial setup\n")
        invalid = self.commit(b"feat: query\n\nbody\n", first)
        self.assertNotEqual(self.policy("history", invalid, "--base", first).returncode, 0)
        left = self.commit(b"feat: left\n", first)
        right = self.commit(b"feat: right\n", first)
        merge = self.commit(b"chore: combine\n", left, right)
        result = self.policy("history", merge, "--base", first)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("merge commit forbidden", result.stderr)

    def test_empty_range_and_missing_base(self):
        first = self.commit(b"chore: initial setup\n")
        result = self.policy("history", first, "--base", first)
        self.assertEqual(result.returncode, 0)
        self.assertIn("no commit messages were checked", result.stdout)
        self.assertNotEqual(self.policy("history", first, "--base", "f" * 40).returncode, 0)

    def event(self, name, data):
        event_file = self.root / "event.json"
        event_file.write_text(json.dumps(data))
        env = dict(self.env, GITHUB_EVENT_NAME=name, GITHUB_EVENT_PATH=str(event_file))
        return self.policy("ci", env=env)

    def test_push_first_commit_and_incremental(self):
        first = self.commit(b"chore: initial setup\n")
        result = self.event("push", dict(ref="refs/heads/main", before="0" * 40, after=first))
        self.assertEqual(result.returncode, 0, result.stderr)
        second = self.commit(b"feat: query\n", first)
        result = self.event("push", dict(ref="refs/heads/feat/query", before=first, after=second))
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_pr_checks_feature_head_not_synthetic_merge(self):
        first = self.commit(b"chore: initial setup\n")
        second = self.commit(b"feat: query\n", first)
        synthetic = self.commit(b"Merge PR #1\n", first, second)
        self.git("update-ref", "refs/heads/main", synthetic)
        pr = dict(title="feat: query", head=dict(ref="feat/query", sha=second), base=dict(sha=first))
        result = self.event("pull_request", dict(pull_request=pr))
        self.assertEqual(result.returncode, 0, result.stderr)
        pr["title"] = "not conventional"
        self.assertNotEqual(self.event("pull_request", dict(pull_request=pr)).returncode, 0)

    def test_shallow_clone_rejected(self):
        first = self.commit(b"chore: initial setup\n")
        self.git("update-ref", "refs/heads/main", first)
        clone = self.root / "shallow"
        self.git("clone", "--depth=1", self.root.as_uri(), str(clone))
        result = subprocess.run([sys.executable, POLICY, "history", first], cwd=clone,
                                env=self.env, capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("full clone", result.stderr)


if __name__ == "__main__":
    unittest.main(verbosity=2)
