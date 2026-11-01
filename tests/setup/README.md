# Independent setup acceptance

This directory is owned by QA. It validates scaffolding and Git policy, not file
transfer or PoT functionality. It does not depend on ignored role documents.

`/Users/lance/Developer/CTSF/tests/setup/git_policy_cases.json` contains 29 full
commit-message inputs, 26 branch inputs and eight history cases. The histories
include a first commit, earlier invalid commits hidden by a valid head, a real
two-parent merge with a valid subject, an empty range, and an invalid ancestor
outside the selected range. A normal final LF is accepted; body, footer and
CoAuthor variants are rejected. All 11 permitted types have positive cases.

`/Users/lance/Developer/CTSF/tests/setup/run_git_policy.py` uses only Python's
standard library and Git. It invokes policy scripts using argument arrays, never
a shell. Message and branch cases use pure inputs. History cases create objects
only in disposable temporary repositories, using `commit-tree`; they do not
stage files, switch the actual checkout or write Git configuration. Inherited
`GIT_*` variables and global/system Git config are excluded from these subprocesses.
The actual project repository is not a Git mutation target.

Before SETUP-B handoff, only validate the fixture structure:

```sh
python3 /Users/lance/Developer/CTSF/tests/setup/run_git_policy.py --check-fixtures
```

This prints `policy_executed: false`; it is not a policy test result. After
handoff, QA supplies an adapter JSON outside production configuration, mapping
`message`, `branch` and `history` to the delivered script's actual argv. Available
placeholders are `{project_root}`, `{repo}`, `{message_file}`, `{branch}`,
`{base}` and `{head}`. Each command runs with a temporary working directory.
Commands must use absolute script paths or `{project_root}`. A root history uses
an all-zero base; an empty range has equal base and head. An optional
`history_root` argv array can omit the base argument for scripts that represent
initial history without a base. Do not guess script
subcommands before inspecting the delivered interface.

The adapter also declares `rejected_exit_codes` (default `[1]`); crashes, missing
executables and timeouts are errors, not expected rejections. An adapter is
required for execution, and missing adapters do not produce a green skipped run.
The runner writes per-case argv, stdout/stderr and exit status to a JSON report
when passed `--report` with a writable absolute file path. It returns nonzero on
any unexpected result. Preserve failure evidence before rerunning after fixes.

The prepared `/Users/lance/Developer/CTSF/tests/setup/git_policy_adapter.json`
maps the currently delivered CLI. After handoff, execute:

```sh
python3 /Users/lance/Developer/CTSF/tests/setup/run_git_policy.py --adapter /Users/lance/Developer/CTSF/tests/setup/git_policy_adapter.json
python3 /Users/lance/Developer/CTSF/tests/setup/test_git_policy_context.py
python3 /Users/lance/Developer/CTSF/tests/setup/test_repository_config.py
```

`/Users/lance/Developer/CTSF/tests/setup/test_git_policy_context.py` adds explicit
hook calls, unborn-repo status, shallow/missing-base rejection, initial/deleted
pushes and PR title/feature-head/synthetic-merge contexts. It uses temporary repos
and event JSON, not hosted GitHub Actions. The hook and policy are copied only to
the temporary repo for hook testing; no hooks are installed.

`/Users/lance/Developer/CTSF/tests/setup/test_repository_config.py` verifies ignore
and attribute behavior through real Git in an isolated repo with copied root
configuration, plus EditorConfig defaults. It does not alter the actual repo's
configuration or local exclude file.

README independence, exact toolchain/dependency checks, real builds and
formatting/type/test failure propagation remain separate setup checklist items
that need the complete delivered build inputs. In particular, an empty
range's zero exit code must be supplemented with evidence that zero commits were
examined; it does not establish that commit messages passed. Detached HEAD and
GitHub synthetic refs are event contexts, not normal feature branch names.

Current state: prepared cases and runner only. No policy implementation has been
executed by this preparation, no hosted CI has run, and no PoT function has passed.
