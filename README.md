# CTSF

CTSF is starting with a shared Rust kernel and a macOS native interface. This setup
provides stateless version/health queries, a headless probe, and a native SwiftUI
application connected through UniFFI. It does not execute transfers. Linux GUI,
persistence, scheduling and backup are outside this stage.

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
python3 scripts/dev.py macos-smoke
python3 scripts/git_policy.py local
```

For a clean source copy, including uncommitted working files, run
`python3 scripts/source_snapshot.py /absolute/path/to/new-directory`. The destination
must not exist; when placed inside this repository, use ignored `build/`.
It copies Git-visible working files without staging or writing Git metadata and
records SHA-256 per source file. It excludes ignored docs and build caches.

`format`, `lint`, `typecheck`, `test` and `smoke` check Rust on macOS/Linux.
`bindings` builds the host Rust FFI static/dynamic libraries and runs this workspace's
version-matched generator. Generated Swift/header/modulemap files go to
`apps/macos/Generated/`; the app must link `target/release/libkernel_ffi.a`.
The `.dylib` is only a generation input, not an app runtime dependency.

The native CI job runs the same `swift-format --check`, `macos-build` and
`macos-smoke` commands. The first checks handwritten Swift style; the second
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
Git configuration is automatically installed. Setup does not configure remotes;
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
