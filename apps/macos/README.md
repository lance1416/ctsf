# macOS setup application

The `CTSFSetup` shared scheme builds a native SwiftUI application that displays
the actual Rust kernel version, setup contract and library health. Both queries
are stateless and perform no I/O; this stage has no execution service or handles.

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
