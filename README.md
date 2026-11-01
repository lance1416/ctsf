# CTSF

CTSF is starting with a shared Rust kernel and a macOS native interface. This setup
contains only a kernel version query and its headless/UniFFI adapters. It does not
execute transfers. Linux GUI, persistence, scheduling and backup are outside this stage.

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
python3 scripts/dev.py bindings
python3 scripts/dev.py swift-format --check
python3 scripts/dev.py macos-build
python3 scripts/git_policy.py local
```

For a clean source copy before any initial commit, run
`python3 scripts/source_snapshot.py /absolute/path/to/new-directory`. The destination
must not exist; when placed inside this repository, use ignored `build/`.
It copies Git-visible working files without staging or writing Git metadata and
records SHA-256 per source file. It excludes ignored docs and build caches.

`format`, `lint`, `typecheck`, `test` and `smoke` check Rust on macOS/Linux.
`bindings` builds the host Rust FFI static/dynamic libraries and runs this workspace's
version-matched generator. Generated Swift/header/modulemap files go to
`apps/macos/Generated/`; the app must link `target/release/libkernel_ffi.a`.
The `.dylib` is only a generation input, not an app runtime dependency.

The macOS application is a subsequent setup handoff. Until its shared Xcode project
and scheme exist, `swift-format` and `macos-build` fail with an explicit prerequisite
error. They do not report an empty/skip check as a pass. Current CI checks Git policy
and Rust on macOS/Linux. The native application feature must introduce its native
CI job together with its sources, using these same commands.

`Cargo.lock` is source-controlled; all repeat builds use `--locked`. Do not run
dependency updates concurrently in this shared checkout. Build output, generated
bindings, user IDE state, and `docs/` are ignored; shared schemes remain trackable.
Build instructions and generation do not depend on `docs/`.

Git rules and the optional explicit hook install are in CONTRIBUTING.md. No hook or
Git configuration is automatically installed. Setup does not configure remotes;
Project agents may push validated changes; the project lead coordinates pushes.
All other remote writes remain reserved for Lance. Small changes integrate into
`main` after validation; features require Lance's acceptance before integration.
Local workflow/Dependabot files do not mean hosted checks have run.
The user-configured origin is `git@github.com:lance1416/ctsf.git`.

## Native setup handoff

SETUP-C supplies handwritten Swift sources, a shared scheme and
`apps/macos/build.json` with these required fields (the paths/names below are an
example for the native owner to choose, not an existing project):

```json
{
  "project": "apps/macos/CTSF.xcodeproj",
  "scheme": "CTSF",
  "deploymentTarget": "15.0",
  "swiftVersion": "6"
}
```

`macos-build` uses the current host architecture, the selected Xcode and SDK,
Swift 6 with complete concurrency checking and warnings as errors, unsigned
Debug output in `build/macos/`, and the same deployment target for Rust and Swift.
The native owner must confirm that target and the static-link settings. No
deployment support claim follows from the example. Generated Swift is excluded
from style rewriting but must be compiled by the native target.
