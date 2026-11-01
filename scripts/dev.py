#!/usr/bin/env python3
"""Shared local/CI checks. Missing prerequisites always fail."""

import argparse
import json
import os
from pathlib import Path
import platform
import shlex
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]


def run(*command, capture=False, env=None):
    print("+ " + shlex.join(str(arg) for arg in command), flush=True)
    result = subprocess.run(command, cwd=ROOT, check=True, text=True,
                            capture_output=capture, env=env)
    return result.stdout if capture else None


def macos():
    if platform.system() != "Darwin":
        raise ValueError("this command requires macOS with full Xcode")


def bindings(env=None):
    macos()
    run("cargo", "build", "--locked", "--release", "-p", "kernel-ffi", env=env)
    output = ROOT / "apps/macos/Generated"
    run("cargo", "run", "--locked", "-p", "uniffi-bindgen", "--", "generate",
        "--library", str(ROOT / "target/release/libkernel_ffi.dylib"),
        "--language", "swift", "--out-dir", str(output), env=env)
    print(f"Bindings: {output}\nStatic library: {ROOT / 'target/release/libkernel_ffi.a'}")


def swift_format(check):
    macos()
    files = sorted(path for path in (ROOT / "apps/macos").rglob("*.swift")
                   if not any(part in {"Generated", "DerivedData", "build"}
                              for part in path.parts))
    if not files:
        raise ValueError("SETUP-C required: no handwritten macOS Swift sources exist")
    if check:
        run("swift", "format", "lint", "--strict", "--configuration",
            str(ROOT / ".swift-format"), *map(str, files))
    else:
        run("swift", "format", "format", "--in-place", "--configuration",
            str(ROOT / ".swift-format"), *map(str, files))


def macos_build():
    macos()
    config_path = ROOT / "apps/macos/build.json"
    if not config_path.is_file():
        raise ValueError("SETUP-C required: apps/macos/build.json must declare project, "
                         "scheme, deploymentTarget, swiftVersion")
    config = json.loads(config_path.read_text())
    project = ROOT / config["project"]
    if not project.is_dir() or project.suffix != ".xcodeproj":
        raise ValueError("SETUP-C required: configured Xcode project does not exist")
    if config["swiftVersion"] != "6":
        raise ValueError("native setup requires Swift 6 language mode")
    run("xcodebuild", "-version")
    run("swift", "--version")
    run("xcrun", "--sdk", "macosx", "--show-sdk-version")
    build_env = os.environ.copy()
    build_env["MACOSX_DEPLOYMENT_TARGET"] = config["deploymentTarget"]
    bindings(env=build_env)
    run("xcodebuild", "-project", str(project), "-scheme", config["scheme"],
        "-configuration", "Debug", "-destination", "platform=macOS",
        "-derivedDataPath", str(ROOT / "build/macos"), "CODE_SIGNING_ALLOWED=NO",
        "SWIFT_VERSION=6", "SWIFT_STRICT_CONCURRENCY=complete",
        "SWIFT_TREAT_WARNINGS_AS_ERRORS=YES", f"ARCHS={platform.machine()}",
        f"MACOSX_DEPLOYMENT_TARGET={config['deploymentTarget']}", "build", env=build_env)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["doctor", "format", "lint", "typecheck", "test",
                                           "smoke", "bindings", "swift-format", "macos-build"])
    parser.add_argument("--check", action="store_true", help="check formatting without editing")
    args = parser.parse_args()
    if args.check and args.command not in {"format", "swift-format"}:
        parser.error("--check is only valid for format and swift-format")
    try:
        if args.command == "doctor":
            print(f"OS: {platform.platform()} / {platform.machine()}")
            print(f"Python: {sys.version}")
            run("git", "--version")
            run("rustup", "show", "active-toolchain")
            run("rustc", "--version", "--verbose")
            run("cargo", "--version")
            run("cargo", "fmt", "--version")
            run("cargo", "clippy", "--version")
            if platform.system() == "Darwin":
                run("xcodebuild", "-version")
                run("swift", "--version")
                run("swift", "format", "--version")
        elif args.command == "format":
            run("cargo", "fmt", "--all", *(["--check"] if args.check else []))
        elif args.command == "lint":
            run("cargo", "clippy", "--locked", "--workspace", "--all-targets", "--", "-D", "warnings")
        elif args.command == "typecheck":
            run("cargo", "check", "--locked", "--workspace", "--all-targets")
        elif args.command == "test":
            run("cargo", "test", "--locked", "--workspace")
            run(sys.executable, "scripts/test_git_policy.py")
        elif args.command == "smoke":
            output = run("cargo", "run", "--locked", "--quiet", "-p", "kernel-probe", capture=True)
            metadata = json.loads(run("cargo", "metadata", "--locked", "--no-deps",
                                      "--format-version", "1", capture=True))
            version = next(package["version"] for package in metadata["packages"]
                           if package["name"] == "kernel")
            expected = f"ctsf-kernel version={version} contract=setup.v1 health=ready\n"
            if output != expected:
                raise ValueError(f"probe output mismatch: {output!r}")
            print(output, end="")
        elif args.command == "bindings":
            bindings()
        elif args.command == "swift-format":
            swift_format(args.check)
        else:
            macos_build()
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        print(f"Check failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
