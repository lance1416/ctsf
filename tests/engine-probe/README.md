# Independent engine-probe acceptance

Build the real probe through the root build entry, then run:

```sh
python3 tests/engine-probe/acceptance.py \
  --probe "$PWD/target/debug/kernel-probe" \
  --engine /usr/bin/rsync \
  --report "$PWD/build/engine-probe/qa-report.json" \
  --expected-version 0.1.0 --source-id <source-identifier>
```

All three paths are mandatory and absolute. A missing real engine fails rather
than skipping. The harness never builds, changes dependencies, runs a transfer,
or modifies the selected real engine. Its only production probe entry is
`inspect-engine`; the legacy health and pure `plan` regressions are separate.

`--check-fixtures` parses the handwritten fixtures/helper and reports
`PREPARED_ONLY`, `probeExecuted=false`. It is not acceptance. The normal report
contains `status`, `checks`, aggregate `counts`, actual environment, binary and
fixture hashes, a source identifier, and retained evidence paths. Assertion or
watchdog failures return nonzero. Expected product errors can pass an acceptance
case when their code, partial observations, and cleanup satisfy the contract.

`fixtures/engine-probe/observations.json` supplies handwritten banner/help text
and capability expectations. The runner does not implement or import the
production identity/help parser. It verifies evidence references against the
actual raw stream and uses exact handwritten line references for the baseline.
The two fixed production arguments remain `--version`, then `--help`.

The one Python standard-library helper is copied into a new case directory with
an absolute interpreter shebang; it privately reads its sibling scenario file.
It records actual argv, environment, cwd, stdin EOF, PID/PGID and readiness. Its
private control protocol is never consumed by production. No engine path or
scenario is inferred from arbitrary user data. Source/destination sentinels are
checked for content, entries, permission and mtime changes.

QA first launches the fixture directly with exactly the four contractual
environment entries. On macOS, Python/runtime may add `__CF_USER_TEXT_ENCODING`;
that one key is accepted only with the same independently measured value and is
reported as fixture runtime augmentation. Raw observations remain available.
The production probe receives only a synthetic parent environment with sentinel
variables, avoiding collection of user environment variables or credentials.

Outer probe stdout/stderr are independently bounded to 512 KiB each. Every probe
invocation has a fixed monotonic eight-second watchdog. A watchdog kills only
the harness's own Popen child. Private fixture stop files let live helpers stop
themselves, avoiding delayed signals to stale numeric process groups. Any such
fallback is reported separately and fails product-cleanup acceptance. Orphan
zombies can yield the specified `CleanupUnconfirmed`; the inherited-pipe case
accepts that honest higher-priority error, while requiring no live fixture.

Process observation inspects only helper-recorded PIDs (`/proc` on Linux, `ps`
on macOS). Inability to independently observe a fixture process is a failing
limitation, not a fabricated cleanup pass. No TCC/system changes are made.

The suite covers exact argv/env/cwd, identity/compatibility versions, four ordered
capabilities, original bytes/UTF-8/evidence, per-stream limits and dual flood,
nonzero/signal exits, one shared deadline, TERM-to-KILL, parent exit with inherited
pipes, metadata change, paths/symlinks, a mandatory actual installed engine, and
legacy behavior. It does not inject OS fcntl/read/wait failures or permission
changes for EPERM, cannot send NUL inside OS argv, and does not prove a hostile
executable sandbox. Kernel direct-API/internal tests cover those applicable
branches. Root must separately retain full core119/native27/workspace regression
and execute this same suite on actual macOS and Linux source/lock snapshots.
