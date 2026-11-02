# CTSF

CTSF is starting with a shared Rust kernel and a macOS native interface. This setup
provides stateless version/health queries, a headless probe, and a native SwiftUI
application connected through UniFFI. The kernel also validates local tasks and
creates argument plans under `planner.v1`. This feature adds a native planning
form and parameter preview through typed UniFFI. The headless probe also inspects
an explicitly selected engine under `engine-probe.v1`. It does not execute transfers.
Linux GUI, persistence, scheduling and backup are outside this stage.

## Requirements

- Rust pinned by `rust-toolchain.toml`, with rustfmt and Clippy. Rustup installs it
  on first use; a cold build needs access to Rust distribution and crates.io.
- Python 3.9+ and Git for the development entry points and Git policy.
- For macOS native work: full Xcode, selected via `xcode-select`, including Swift
  compiler and the toolchain's `swift format` command. Swift style checks and
  compiler type/concurrency checks are separate.
- Linux Rust checks require a native linker, but no Apple tools or desktop libraries.

## Commands

Run from this directory:

```sh
python3 scripts/dev.py doctor
python3 scripts/dev.py format --check
python3 scripts/dev.py lint
python3 scripts/dev.py typecheck
python3 scripts/dev.py test
python3 scripts/dev.py smoke
python3 scripts/dev.py engine-probe-acceptance --engine /usr/bin/rsync
python3 scripts/dev.py bindings
python3 scripts/dev.py swift-format --check
python3 scripts/dev.py macos-build
python3 scripts/dev.py macos-smoke
python3 scripts/dev.py macos-planner-smoke
python3 scripts/git_policy.py local
```

For a clean source copy, including uncommitted working files, run
`python3 scripts/source_snapshot.py /absolute/path/to/new-directory`. The destination
must not exist; when placed inside this repository, use ignored `build/`.
It copies Git-visible working files without staging or writing Git metadata and
records SHA-256 per source file. It excludes ignored docs and build caches.

## Local invocation planner

The headless probe accepts `plan <task-json-path>`. Prepare a UTF-8 JSON task file
using this example, replacing the source and destination with existing, separate
local directories. All three task paths must be absolute. The engine path can be
nonexistent: this stage does not inspect or execute it.

```json
{
  "schemaVersion": 1,
  "kind": "rsync.task.pot",
  "taskId": "local-plan-example",
  "revision": 1,
  "name": "Local planning example",
  "engineExecutable": "/not-installed/rsync",
  "source": {"raw": "/absolute/path/to/源 目录/"},
  "destination": {"raw": "/absolute/path/to/target/"},
  "options": [{"kind": "recursive"}]
}
```

From the repository root, build first, then invoke the binary directly so Cargo
diagnostics are separate from the probe's machine-readable streams:

```sh
cargo build --locked -p kernel-probe
./target/debug/kernel-probe plan /absolute/path/to/task.json
```

With no arguments, `./target/debug/kernel-probe` retains the single-line
`setup.v1` version/health output. Planning uses this stream/exit contract:

| Result | Exit | stdout | stderr |
|---|---|---|---|
| Directory checks and planning succeed | 0 | One `InvocationPlan` JSON object and newline | Empty |
| Expected input, task-file or path error | 2 | Empty | One `PlanningError` JSON object and newline |

Unexpected failures also exit nonzero. Exit 0 means a parameter plan was built;
it does not mean a transfer or a real Dry Run succeeded. The successful JSON has
these fields:

| Field | Meaning |
|---|---|
| `contractVersion` | `"planner.v1"` |
| `taskSnapshot` | An owned copy of the validated task |
| `executable` | The original `engineExecutable` string |
| `runArgv` | `["--recursive", "--", source.raw, destination.raw]` for the example |
| `dryRunArgv` | `["--recursive", "--dry-run", "--", source.raw, destination.raw]` |
| `pathCheck` | `"checked_at_planning"` |
| `engineCheck` | `"not_performed"` |

In the argument descriptions above, `source.raw` and `destination.raw` each stand
for one complete string token, including spaces, Chinese text, literal shell
characters and trailing `/`. Neither array includes the executable. The only
Run/Dry Run difference is the inserted `--dry-run` token. No shell command is
constructed or run.

An error object contains `code`, nullable `field`, explanatory `message` and
nullable `osCode`. Handle the code/field instead of parsing the message. For
example, the same-directory error has `code="unsafe_path_relation"` and
`field="destination.raw"`. Error codes are `invalid_json`, `unsupported_schema`,
`invalid_task`, `invalid_path`, `unsafe_path_relation`, `path_access_denied` and
`path_io`. Task-file errors identify `taskFile`; usage errors identify `arguments`.

Tasks are limited to 64 KiB of UTF-8 JSON, with all fields required. Unknown,
duplicate, missing or wrongly typed fields and unknown options are rejected.
`schemaVersion` must be 1 and `revision` at least 1. `taskId` is 1–128 UTF-8 bytes
and `name` 1–256; neither may be all whitespace or contain NUL.
`options` must contain exactly one `recursive` or `archive` object, with no extra
fields; archive substitutes `--archive` in both arrays. Paths are 1–4096 UTF-8
bytes, absolute and NUL-free. The planner does not trim or expand paths, or alter
trailing slashes. JSON round trips preserve values and array order, not formatting
or object-key order.

Planning reads source/destination directory metadata and resolves top-level
aliases for checks. It rejects roots, missing/non-directory paths, identical
directories and nesting in either direction, including symlink aliases. Resolved
paths are not substituted into argv. Checks do not create directories, write
files, probe write access or execute the engine. A successful check records the
filesystem state at planning time; it does not lock directories or authorize a
future execution. Engine capability and permissions remain unchecked.

This stage adds no process runner, runtime, execution IDs, cancellation, execution logs,
SSH, deletion/filter/bandwidth options, `--stats` or `--itemize-changes`. The task
kind does not imply those future capabilities are implemented. The native form
submits typed tasks to a serial worker, then displays both returned argv arrays.
Editing invalidates the preview and disables copying; valid Copy actions encode
the returned executable/arguments as JSON. The existing library health display
remains separate from engine readiness. Run/Dry Run labels describe plans only.

## Controlled engine inspection

The headless probe accepts an explicit absolute engine path:

```sh
cargo build --locked -p kernel-probe
./target/debug/kernel-probe inspect-engine /usr/bin/rsync
```

It runs only `--version` followed by `--help`, without source/destination arguments
or a shell. Both steps have bounded stdout/stderr capture and share a three-second
runtime deadline, with separate cleanup budgets. This is a short synchronous
inspection; it adds no runtime, transfer, cancellation API or native UI action.

Successful observation writes one `engine-probe.v1` JSON envelope to stdout and
exits 0: `status` is `ok`, `report` contains identity/capability/command evidence,
and `error` is null. A probe error exits 2 with one envelope on stderr, empty stdout,
`status="error"`, structured error details and any partial report. A successful
observation can still have unknown implementation or capabilities; it does not
approve an engine for transfer. Cleanup failures must not appear as success.

The report distinguishes implementation version, compatibility version and
protocol version. Apple openrsync's `2.6.9 compatible` banner is not a GNU rsync
implementation version. Capabilities cover only `--recursive`, `--archive`,
`--dry-run` and `--`, using explicit help evidence; absent long-option evidence
remains unknown, including compact short-option usage. No transfer or real Dry Run
is attempted. Existing planner results continue to say `engineCheck=not_performed`;
inspection is a separate operation and does not change planner semantics.

## Build and checks

`format`, `lint`, `typecheck`, `test` and `smoke` check Rust on macOS/Linux.
After workspace and Git policy tests, `test` explicitly builds `kernel-probe` and
runs `tests/planner/acceptance.py` against that executable. Cargo build artifacts
and locked metadata supply its path and expected kernel version. The same command
runs in both Rust CI jobs; any acceptance failure fails `test`. The ignored report
is `build/planner-acceptance.json`, with probe SHA-256 evidence from the QA runner.
The Rust CI jobs separately run `engine-probe-acceptance` against `/usr/bin/rsync`.
That standalone command also accepts `--engine /absolute/path/to/rsync`; omitting
the option still selects `/usr/bin/rsync`, never an implicit PATH lookup. It builds
the real probe using locked Cargo metadata/artifacts and runs the independent QA
runner. Missing engines or test sources fail; the command does not install an
engine or skip acceptance. It removes the previous ignored
`build/engine-probe-acceptance.json` before running and requires fresh, nonempty,
all-PASS checks. Existing planner and native smoke commands remain separate.
`bindings` builds the host Rust FFI static/dynamic libraries and runs this workspace's
version-matched generator. Generated Swift/header/modulemap files go to
`apps/macos/Generated/`; the app must link `target/release/libkernel_ffi.a`.
The `.dylib` is only a generation input, not an app runtime dependency.

The native CI job runs the same `swift-format --check`, `macos-build`,
`macos-smoke` and `macos-planner-smoke` commands. The first checks handwritten Swift
under `apps/macos` and `tests/native-planner`; the second
regenerates bindings and compiles both handwritten and generated Swift, including
type/concurrency checks. The smoke statically links the Swift bridge and checks
the actual kernel version, setup contract and health, repeating both queries 100
times. Its expected version comes from locked Cargo metadata.
Missing tools, configuration or sources fail explicitly. Building the app does not
establish GUI launch or interaction; those checks are recorded separately.

`Cargo.lock` is source-controlled; all repeat builds use `--locked`. Do not run
dependency updates concurrently in this shared checkout. Build output, generated
bindings, user IDE state, and `docs/` are ignored; shared schemes remain trackable.
Build instructions and generation do not depend on `docs/`.

Git rules and the optional explicit hook install are in CONTRIBUTING.md. No hook or
Git configuration is automatically installed. Setup does not configure remotes.
Project agents may push validated changes; the project lead coordinates pushes.
All other remote writes remain reserved for Lance. Small changes integrate into
`main` after validation; features require Lance's acceptance before integration.
Local workflow/Dependabot files do not mean hosted checks have run.
The user-configured origin is `git@github.com:lance1416/ctsf.git`.

## Native macOS build

`apps/macos/build.json` declares the existing Xcode project and shared scheme:

```json
{
  "project": "apps/macos/CTSF.xcodeproj",
  "scheme": "CTSFSetup",
  "deploymentTarget": "14.0",
  "swiftVersion": "6"
}
```

`macos-build` uses the current host architecture, the selected Xcode and SDK,
Swift 6 with complete concurrency checking and warnings as errors, unsigned
Debug output in `build/macos/`, and the same deployment target for Rust and Swift.
The setup deployment target is not a product support matrix. Generated Swift is
excluded from style rewriting and compiled by the native target. The app is written
to `build/macos/Build/Products/Debug/CTSFSetup.app`. See
[the macOS application notes](apps/macos/README.md) for static linking, direct Xcode
usage and the separate local launch check. Hosted CI has not been validated by
this local build.

`macos-smoke` can also run independently: it rebuilds the Rust library, regenerates
bindings, and compiles `apps/macos/Tests/KernelBridgeSmoke.swift` with Swift 6 and
complete concurrency checking. It uses the same deployment target and host
architecture as the app build. Its executable is `build/macos/KernelBridgeSmoke`;
build, assertion or execution failures return nonzero. This checks bridge values
without GUI automation and does not establish leak freedom.

`macos-planner-smoke` independently regenerates bindings, then statically compiles
the generated Swift, four production planner value/worker/coordinator/FFI sources
and the three explicit QA harness sources. It excludes the SwiftUI App/UI and uses
the same deployment target, host architecture and strict Swift 6 settings. The
full typed-bridge and production-worker/coordinator gate receives the absolute
`fixtures/native-planner` path and kernel version from Cargo metadata; `--typed-only`
is a diagnostic mode and is not used by this command or CI.

The JSON report is written to `build/macos/native-planner-report.json`. Missing
sources, compilation/execution failures, failed checks or invalid report JSON
return nonzero. Any `UNVERIFIED` checks remain explicit as `PASS_WITH_LIMITATIONS`
in the report. This harness does not replace GUI accessibility or visual acceptance;
running the command is required before claiming its checks have passed.
