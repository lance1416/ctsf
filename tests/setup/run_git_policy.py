#!/usr/bin/env python3
"""Independent setup policy cases; supply argv adapters after script handoff."""

import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile


HERE = Path(__file__).resolve().parent
PROJECT = HERE.parent.parent
ZERO_SHA = "0" * 40


def isolated_env():
    env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
    env.update(
        GIT_CONFIG_NOSYSTEM="1",
        GIT_CONFIG_GLOBAL=os.devnull,
        GIT_CONFIG_SYSTEM=os.devnull,
        GIT_TERMINAL_PROMPT="0",
        GIT_AUTHOR_NAME="Setup QA",
        GIT_AUTHOR_EMAIL="setup-qa@example.invalid",
        GIT_COMMITTER_NAME="Setup QA",
        GIT_COMMITTER_EMAIL="setup-qa@example.invalid",
        GIT_AUTHOR_DATE="2026-10-31T12:00:00+00:00",
        GIT_COMMITTER_DATE="2026-10-31T12:00:00+00:00",
    )
    return env


def git(repo, env, *args, input_text=None):
    result = subprocess.run(
        ["git", *args], cwd=repo, env=env, input=input_text,
        text=True, capture_output=True, timeout=15, check=True,
    )
    return result.stdout.strip()


def make_history(repo, env, case):
    """Write objects/HEAD only in a new temporary repo, without config or checkout."""
    git(repo, env, "init", "--initial-branch=main", "--template=", ".")
    tree = git(repo, env, "mktree", input_text="")

    def commit(message, parents):
        args = ["commit-tree", tree]
        for parent in parents:
            args.extend(["-p", parent])
        return git(repo, env, *args, input_text=message)

    messages = case["messages"]
    root = commit(messages[0], [])
    shape = case["shape"]
    if shape == "merge":
        left = commit(messages[1], [root])
        right = commit(messages[2], [root])
        head = commit(messages[3], [left, right])
    else:
        head = root
        for message in messages[1:]:
            head = commit(message, [head])
    git(repo, env, "update-ref", "HEAD", head)
    base = ZERO_SHA if shape == "root" else root
    return {"base": base, "head": head}


def load_cases():
    cases = json.loads((HERE / "git_policy_cases.json").read_text(encoding="utf-8"))
    assert cases["version"] == 1
    ids = set()
    for group in ("messages", "branches", "histories"):
        assert cases[group], group
        for case in cases[group]:
            assert case["id"] not in ids, case["id"]
            ids.add(case["id"])
            assert type(case["accept"]) is bool, case["id"]
            if group != "histories":
                assert isinstance(case["value"], str), case["id"]
            else:
                shape = case["shape"]
                assert shape in ("root", "linear", "merge", "empty"), case["id"]
                assert len(case["messages"]) == {"root": 1, "merge": 4, "empty": 1}.get(
                    shape, len(case["messages"])
                ), case["id"]
    return cases


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check-fixtures", action="store_true")
    parser.add_argument("--adapter", type=Path)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    cases = load_cases()
    if args.check_fixtures:
        print(json.dumps({
            "status": "FIXTURES_VALIDATED_ONLY",
            "policy_executed": False,
            "counts": {key: len(cases[key]) for key in ("messages", "branches", "histories")},
        }))
        return 0
    if args.adapter is None:
        parser.error("--adapter is required; missing policy scripts are not a passing test")
    adapter = json.loads(args.adapter.read_text(encoding="utf-8"))
    reject_codes = adapter.get("rejected_exit_codes", [1])
    if not reject_codes or any(type(code) is not int or code <= 0 for code in reject_codes):
        parser.error("rejected_exit_codes must contain positive process exit codes")
    for operation in ("message", "branch", "history"):
        argv = adapter.get(operation)
        if not isinstance(argv, list) or not argv or not all(isinstance(x, str) for x in argv):
            parser.error("adapter must supply nonempty argv arrays for message, branch, history")
    results = []
    env = isolated_env()
    for group, operation in (("messages", "message"), ("branches", "branch"), ("histories", "history")):
        for case in cases[group]:
            with tempfile.TemporaryDirectory(prefix="ctsf-setup-policy-") as directory:
                repo = Path(directory)
                values = {"project_root": str(PROJECT), "repo": str(repo)}
                try:
                    if operation == "message":
                        message_file = repo / "COMMIT_EDITMSG"
                        message_file.write_bytes(case["value"].encode("utf-8"))
                        values["message_file"] = str(message_file)
                    elif operation == "branch":
                        values["branch"] = case["value"]
                    else:
                        values.update(make_history(repo, env, case))
                    template = adapter.get("history_root", adapter[operation]) if (
                        operation == "history" and case["shape"] == "root"
                    ) else adapter[operation]
                    argv = [token.format_map(values) for token in template]
                    completed = subprocess.run(
                        argv, cwd=repo, env=env, capture_output=True, text=True, timeout=30,
                    )
                    valid_exit = completed.returncode == 0 if case["accept"] else completed.returncode in reject_codes
                    crashed = "Traceback (most recent call last)" in completed.stderr
                    results.append({
                        "caseId": case["id"], "expectedAccept": case["accept"],
                        "status": "PASS" if valid_exit and not crashed else "FAIL",
                        "argv": argv, "exitCode": completed.returncode,
                        "stdout": completed.stdout, "stderr": completed.stderr,
                    })
                except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
                    results.append({"caseId": case["id"], "status": "ERROR", "error": str(error)})
    report = {
        "scope": "setup_git_policy_only", "policy_executed": True,
        "status": "PASS" if all(result["status"] == "PASS" for result in results) else "FAIL",
        "results": results,
    }
    rendered = json.dumps(report, ensure_ascii=False, indent=2) + "\n"
    if args.report is not None:
        args.report.write_text(rendered, encoding="utf-8")
    else:
        print(rendered, end="")
    print(json.dumps({"status": report["status"], "cases": len(results)}))
    return 0 if report["status"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
