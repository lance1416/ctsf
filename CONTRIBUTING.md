# Contributing

Use a single-line Conventional Commit subject, for example `chore(repo): scaffold tooling`.
Allowed types: feat, fix, perf, refactor, docs, test, build, ci, chore, style, revert.
Scope is optional; `!` is allowed before the colon. Do not add a body, footer, trailer,
or CoAuthor attribution. One final line ending is allowed.

Work branches use `type/kebab-case-description`, for example `feat/task-editor` or
`chore/project-setup`. `main` is reserved. Coordinate branch changes in a shared checkout.
Keep history linear: rebase then fast-forward, or squash using the same message rules.
Merge commits are rejected. Do not change global Git configuration.

Validated local changes should be committed as part of normal delivery. In a shared
checkout, the project lead coordinates the index, branch changes and commit owner;
other roles must not commit concurrently. Local commit authority does not authorize
any remote write. Lance manages pushes.

Remote access by project agents is read-only. Lance manages every push and all
remote changes. Agents must not push/force-push, create or change remote branches
or tags, create/edit/merge pull requests, publish releases, change repository or
branch-protection settings, or trigger/rerun remote workflows. Writing a local
workflow or Dependabot configuration does not authorize activating it remotely.

The optional hook can be explicitly installed by the developer:

```sh
git config --local core.hooksPath .githooks
```

This command changes local Git configuration; the setup process does not run it.
Without hook installation, validate a complete message file with
`python3 scripts/git_policy.py message /absolute/path/to/message`.
Run `python3 scripts/git_policy.py local` for the current branch/history. An unborn
HEAD is reported as not checked, not as a passing commit history. CI checks the
actual PR head and full introduced messages, PR title, branch name and merge commits.
CI requires full history. Missing history is an error, never a silent success.

Run the checks in README before proposing changes. `docs/` is intentionally ignored;
do not force-add it or add an unignore rule. The source build must work without it.

Lance configured `origin` as `git@github.com:lance1416/ctsf.git`; agents only read it.
Dependabot configuration monitors Cargo, GitHub Actions and the Rust toolchain
weekly, Monday 09:00 Europe/London, with up to three version-update PRs per ecosystem.
Version/security updates are grouped separately. Fixed branch templates render
`chore/cargo-updates`, `ci/actions-updates` and `chore/toolchain-updates`; the grouped
update digest suffix also fits the branch rule. Conventional subject prefixes are
configured. There is no bot exemption or automatic merge.

The configuration cannot guarantee a body/trailer-free bot commit: the documented
commit-message options customize prefixes/scope, not complete raw message bodies.
Strict full-message checks remain active. A noncompliant proposal must be locally
cleaned or squashed into a single-line commit on a compliant branch; project agents
may prepare the local commit, and Lance manages the push. Generated refs/messages
and service-side acceptance of this configuration
have not been observed; local template checks do not establish hosted behavior.
See the [official Dependabot options](https://docs.github.com/en/code-security/reference/supply-chain-security/dependabot-options-reference).
