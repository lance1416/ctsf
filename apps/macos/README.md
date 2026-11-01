# macOS setup application

The `CTSFSetup` shared scheme builds a native SwiftUI application with the actual
Rust kernel version, setup contract and library health, followed by a local task
form and two invocation previews. The health queries are stateless and perform
no I/O. Generating a plan reads existing directory metadata through the Rust
planner; it never starts rsync, including Dry Run, or writes transfer data.

Enter an absolute engine path, select or enter two existing independent local
test directories, and explicitly select Recursive or Archive. Paths start empty;
the option starts unselected. Directory selection preserves the selected path
without adding/removing a trailing slash. Keyboard input is also passed raw.
An engine path need not exist: engine existence, permissions, version and option
support remain unchecked at this stage.

Click **Generate plan** (Command-Shift-P) to generate both **Run argv (not executed)**
and **Dry Run argv (not executed)**. They show the returned program separately from
the ordered arguments. The two Copy buttons copy the returned program and selected
argument array as JSON, never as a shell command. A directory check only describes
the filesystem at planning time; it does not prove transfer permissions or lock
the directories.

Only one planning request may be in flight. Editing stays available and makes
any old result stale, disables copying, and increments the form revision even
when a path is changed back. An older response cannot replace the current input's
result. Complete the current request before explicitly generating again. Closing
the window does not wait for directory I/O or introduce a transmission shutdown
flow; a late response cannot revive the closed window.

Use the selected full Xcode toolchain and run from the repository root:

```sh
python3 scripts/dev.py swift-format --check
python3 scripts/dev.py macos-build
```

The build entry point reads `apps/macos/build.json`, builds the Rust library,
regenerates UniFFI bindings, and compiles the app with Swift 6, complete concurrency
checking and warnings as errors. The deployment target is macOS 14.0 for this
setup build; only the current host architecture is built and tested. This is not
a product support matrix.

Generated Swift, its FFI header and modulemap belong in `apps/macos/Generated/`.
They are ignored build outputs; do not edit or format them. The project compiles
the generated Swift, imports the generated C modulemap, and links the exact
`target/release/libkernel_ffi.a` archive. The `.dylib` is only an input to the
version-matched workspace generator, never an app runtime dependency. There are
no Xcode script phases that duplicate the root build entry point.

The production common sources are `PlannerValues.swift`, `PlannerWorker.swift`
and `PlannerCoordinator.swift`. They contain no SwiftUI or app entry point, so an
independent harness can compile the same token/revision gate and private serial
Dispatch worker. `PlannerFFI.plan(_:)` is the actual synchronous operation passed
to that worker; it constructs/consumes generated DTOs off the main thread and
returns application-owned immutable Sendable values. `PlannerUI.swift` handles
the native form and clipboard presentation. Generated DTOs never become live
SwiftUI bindings, and Swift does not rebuild the Rust argument arrays.

The worker accepts an injectable Sendable operation and the coordinator accepts
an injectable async provider for deterministic scheduling checks. These are Swift
implementation seams, not Rust/FFI debug exports. Request/snapshot comparisons
use UTF-8 bytes, so NFC/NFD changes also invalidate the preview. The existing setup
smoke below remains a health regression; independent planner bridge/gate evidence
is tracked separately and cannot be inferred from the setup smoke alone.

Run the unified bridge smoke entry point from the repository root; the native CI
job uses the same command:

```sh
python3 scripts/dev.py macos-smoke
```

The runner derives
the kernel version from Cargo metadata and the host/deployment settings from the
build configuration, so callers need no manual Swift flags or version argument.
The underlying `Tests/KernelBridgeSmoke.swift` has already been compiled and run
with Swift 6 and the generated binding/static archive. It checks the actual
version/contract and health enum, then repeats both queries 100 times. It is
separate from the app target and does not prove GUI control values or leak freedom.

The unsigned Debug build is written to:

```text
build/macos/Build/Products/Debug/CTSFSetup.app
```

After a successful build, launch it locally with:

```sh
open build/macos/Build/Products/Debug/CTSFSetup.app
```

Building does not prove GUI launch. Local launch is a separate check, subject to
the host's normal launch and signing requirements. No Developer ID, store
distribution or notarization is configured. When opening the Xcode project
directly, first run `python3 scripts/dev.py macos-build` to prepare the matching
Rust archive and bindings.
