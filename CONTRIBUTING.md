# Contributing

Use a single-line Conventional Commit subject, for example `chore(repo): scaffold tooling`.
Allowed types: feat, fix, perf, refactor, docs, test, build, ci, chore, style, revert.
Scope is optional; `!` is allowed before the colon. Do not add a body, footer, trailer,
or CoAuthor attribution. One final line ending is allowed.

Work branches use `type/kebab-case-description`, for example `feat/task-editor` or
`chore/project-setup`. `main` is reserved. Coordinate branch changes in a shared checkout.
Keep history linear: rebase then fast-forward, or squash using the same message rules.
Merge commits are rejected. Validated small changes are automatically rebased onto
`main` and integrated by fast-forward. Features require Lance's acceptance before
rebasing onto and integrating into `main`. Do not change global Git configuration.

Validated local changes should be committed as part of normal delivery. In a shared
checkout, the project lead coordinates the index, branch changes and commit owner;
other roles must not commit concurrently. The project lead coordinates pushes.

Project agents may use Git push; all other remote writes remain reserved for Lance.
For the current main CI repair, Lance explicitly authorized rewriting the affected
commit and pushing with `--force-with-lease`; fold corrections into that commit
instead of leaving a separate CI repair commit. Use an explicit expected remote
SHA for the lease. This permission does not authorize remote branch/tag deletion,
pull-request creation/editing/merging, publishing releases, repository or branch
protection changes, or explicit workflow triggering/rerunning. Push-triggered CI
may run normally. Future history rewrites still require appropriate authorization.

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
CI requires full history. Forced pushes check the complete surviving history, since
the replaced head may be unavailable; ordinary pushes still require their base.
Missing required history is an error, never a silent success.

Run the checks in README before proposing changes. `docs/` is intentionally ignored;
do not force-add it or add an unignore rule. The source build must work without it.

Lance configured `origin` as `git@github.com:lance1416/ctsf.git`.
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
may prepare and push the local commit under the coordination rules above. Actual
hosted runs must establish generated refs/messages and service-side behavior;
local template checks alone do not establish them.
See the [official Dependabot options](https://docs.github.com/en/code-security/reference/supply-chain-security/dependabot-options-reference).
