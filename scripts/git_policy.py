#!/usr/bin/env python3
"""Project Git rules. No Git configuration or history is modified."""

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys

TYPES = "feat|fix|perf|refactor|docs|test|build|ci|chore|style|revert"
SUBJECT = re.compile(rf"(?:{TYPES})(?:\([a-z0-9][a-z0-9._/-]*\))?!?: [^\s].*")
BRANCH = re.compile(rf"(?:{TYPES})/[a-z0-9]+(?:-[a-z0-9]+)*")
SHA = re.compile(r"[0-9a-f]{40}(?:[0-9a-f]{24})?")


def message_error(raw):
    try:
        text = raw.decode("utf-8")
    except UnicodeDecodeError:
        return "message must be UTF-8"
    # Git's single final line ending is permitted; even an empty second line is a body.
    if text.endswith("\r\n"):
        text = text[:-2]
    elif text.endswith("\n"):
        text = text[:-1]
    if any(char in text for char in "\r\n\x00\v\f\x85\u2028\u2029"):
        return "message must contain exactly one subject line, with no body or trailer"
    if not SUBJECT.fullmatch(text) or text != text.rstrip():
        return "expected type(scope): subject (scope optional; no trailing whitespace)"
    if re.search(r"co-?author(?:ed)?(?:-?by)?\s*:", text, re.IGNORECASE):
        return "CoAuthor attribution is forbidden"
    return None


def require_message(raw):
    error = message_error(raw)
    if error:
        raise ValueError(error)


def require_branch(name):
    if name != "main" and not BRANCH.fullmatch(name):
        raise ValueError("branch must be main or type/kebab-case-description")


def git(*args):
    result = subprocess.run(["git", *args], capture_output=True, check=False)
    if result.returncode:
        raise ValueError(result.stderr.decode(errors="replace").strip() or "Git command failed")
    return result.stdout


def require_commit(sha):
    if not SHA.fullmatch(sha):
        raise ValueError("expected a full hexadecimal commit SHA")
    git("cat-file", "-e", sha + "^{commit}")


def history(head, base=None):
    require_commit(head)
    if git("rev-parse", "--is-shallow-repository").strip() == b"true":
        raise ValueError("history check requires a full clone (fetch-depth: 0)")
    if base:
        require_commit(base)
    revision = f"{base}..{head}" if base else head
    commits = git("rev-list", revision).decode().splitlines()
    for sha in commits:
        raw = git("cat-file", "commit", sha)
        headers, message = raw.split(b"\n\n", 1)
        if sum(line.startswith(b"parent ") for line in headers.splitlines()) > 1:
            raise ValueError(f"merge commit forbidden: {sha}")
        try:
            require_message(message)
        except ValueError as error:
            raise ValueError(f"{sha}: {error}") from error
    if commits:
        print(f"Git policy: checked {len(commits)} complete commit message(s); no merge commits")
    else:
        print("Git policy: empty range; no commit messages were checked")


def ci():
    event_name = os.environ.get("GITHUB_EVENT_NAME")
    event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text())
    if event_name == "pull_request":
        pr = event["pull_request"]
        require_branch(pr["head"]["ref"])
        require_message(pr["title"].encode())
        history(pr["head"]["sha"], pr["base"]["sha"])
    elif event_name == "push":
        if event.get("deleted"):
            print("Git policy: deleted ref; no branch head or commits to check")
            return
        ref = event["ref"]
        if not ref.startswith("refs/heads/"):
            raise ValueError("push policy expects a branch ref")
        require_branch(ref.removeprefix("refs/heads/"))
        base = event["before"]
        # A rewritten branch's previous head may not exist in a fresh checkout.
        # Validate every surviving commit rather than depending on discarded history.
        history(event["after"], None if event.get("forced") or set(base) == {"0"} else base)
    else:
        raise ValueError("supported CI events: push, pull_request")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("message").add_argument("file", type=Path)
    commands.add_parser("branch").add_argument("name")
    range_parser = commands.add_parser("history")
    range_parser.add_argument("head")
    range_parser.add_argument("--base")
    commands.add_parser("ci")
    commands.add_parser("local")
    args = parser.parse_args()
    try:
        if args.command == "message":
            require_message(args.file.read_bytes())
            print("Git policy: message accepted")
        elif args.command == "branch":
            require_branch(args.name)
            print("Git policy: branch accepted")
        elif args.command == "history":
            history(args.head, args.base)
        elif args.command == "ci":
            ci()
        else:
            branch = git("symbolic-ref", "--quiet", "--short", "HEAD").decode().strip()
            require_branch(branch)
            result = subprocess.run(["git", "rev-parse", "--verify", "HEAD"], capture_output=True)
            if result.returncode:
                print("Git policy: branch accepted; UNBORN HEAD — commit checks NOT RUN")
            else:
                history(result.stdout.decode().strip())
    except (ValueError, OSError, KeyError) as error:
        print(f"Git policy: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
