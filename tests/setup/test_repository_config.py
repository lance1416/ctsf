#!/usr/bin/env python3
"""Check editor/Git file policy using copied files and an isolated repository."""

import configparser
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from run_git_policy import PROJECT, git, isolated_env


class RepositoryConfigTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="ctsf-setup-config-")
        self.addCleanup(self.directory.cleanup)
        self.repo = Path(self.directory.name)
        self.env = isolated_env()
        git(self.repo, self.env, "init", "--initial-branch=main", "--template=", ".")
        for name in (".gitignore", ".gitattributes"):
            shutil.copy2(PROJECT / name, self.repo / name)

    def test_ignored_data_and_build_products(self):
        paths = [
            "docs/team/qa.md", "target/release/kernel-probe", "build/macos/App.app",
            "apps/macos/Generated/kernel_ffi.swift",
            "apps/macos/DerivedData/cache", "apps/macos/CTSF.xcodeproj/xcuserdata/user.state",
            "apps/macos/User.xcuserstate", "scripts/__pycache__/cache.pyc", ".DS_Store",
        ]
        ignored = git(self.repo, self.env, "check-ignore", "--no-index", "--stdin",
                      input_text="\n".join(paths) + "\n").splitlines()
        self.assertEqual(set(ignored), set(paths))

    def test_source_lock_and_shared_scheme_are_trackable(self):
        paths = [
            "Cargo.lock", "Cargo.toml", "rust-toolchain.toml", "README.md", "CONTRIBUTING.md",
            "crates/kernel/src/lib.rs", "scripts/dev.py", "tests/setup/run_git_policy.py",
            "apps/macos/CTSF.xcodeproj/xcshareddata/xcschemes/CTSF.xcscheme",
        ]
        result = subprocess.run(
            ["git", "check-ignore", "--no-index", "--stdin"], cwd=self.repo, env=self.env,
            input="\n".join(paths) + "\n", capture_output=True, text=True, timeout=15,
        )
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertEqual(result.stdout, "")

    def test_attributes_text_lf_and_binary(self):
        raw = git(self.repo, self.env, "check-attr", "-z", "text", "eol", "--",
                  "sample.rs", "sample.swift", "sample.toml", "sample.yml", "sample.md",
                  "sample.png", "sample.jpg", "sample.pdf")
        parts = raw.split("\0")
        attributes = {(parts[i], parts[i + 1]): parts[i + 2]
                      for i in range(0, len(parts) - 1, 3)}
        for extension in ("rs", "swift", "toml", "yml", "md"):
            self.assertEqual(attributes[("sample." + extension, "text")], "set")
            self.assertEqual(attributes[("sample." + extension, "eol")], "lf")
        for extension in ("png", "jpg", "pdf"):
            self.assertEqual(attributes[("sample." + extension, "text")], "unset")

    def test_editor_config_defaults(self):
        parser = configparser.ConfigParser(interpolation=None)
        parser.read_string("[configuration]\n" + (PROJECT / ".editorconfig").read_text())
        self.assertTrue(parser.getboolean("configuration", "root"))
        self.assertEqual(parser["*"]["charset"], "utf-8")
        self.assertEqual(parser["*"]["end_of_line"], "lf")
        self.assertTrue(parser.getboolean("*", "insert_final_newline"))
        self.assertEqual(parser["*"]["indent_style"], "space")
        self.assertGreater(int(parser["*"]["indent_size"]), 0)


if __name__ == "__main__":
    unittest.main(verbosity=2)
