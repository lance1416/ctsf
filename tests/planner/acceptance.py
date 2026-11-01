#!/usr/bin/env python3
"""Black-box planner.v1 acceptance. Python standard library only; no builds."""

import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import platform
import stat
import subprocess
import sys
import tempfile
import time


ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / "fixtures/planner"
MAX_JSON = 64 * 1024
REQUIRED_FIELDS = (
    "schemaVersion", "kind", "taskId", "revision", "name", "engineExecutable",
    "source", "destination", "options",
)


def encode(value):
    return json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode("utf-8")


def digest(data):
    return hashlib.sha256(data).hexdigest()


def strict_json(raw):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError("duplicate key in probe output: " + key)
            result[key] = value
        return result
    return json.loads(raw.decode("utf-8"), object_pairs_hook=unique)


def tree_manifest(root):
    result = {}

    def visit(path):
        name = str(path.relative_to(root))
        info = path.lstat()
        node = {"mode": stat.S_IMODE(info.st_mode), "mtimeNs": info.st_mtime_ns}
        if stat.S_ISLNK(info.st_mode):
            node.update(kind="symlink", target=os.readlink(path))
        elif stat.S_ISDIR(info.st_mode):
            node["kind"] = "directory"
            try:
                children = sorted(path.iterdir())
            except PermissionError as error:
                node["listingError"] = error.errno
                children = []
            result[name] = node
            for child in children:
                visit(child)
            return
        else:
            node.update(kind="file", size=info.st_size, sha256=digest(path.read_bytes()))
        result[name] = node

    visit(root)
    return result


class FixtureTree:
    def __init__(self, root):
        self.root = root
        for name in ("src", "src/child", "src2", "dst", "中文 空格", "protected/inner", "engine"):
            (root / name).mkdir(parents=True, exist_ok=True)
        self.shell = root / "源 空格 : $HOME * ? [x] ' \" $(touch shell-marker) `printf literal`"
        self.shell.mkdir()
        (root / "src/payload.txt").write_bytes((FIXTURES / "payload.txt").read_bytes())
        (root / "src/empty").write_bytes(b"")
        (root / "src/child/binary.bin").write_bytes(bytes(range(256)))
        (root / "dst/existing.txt").write_text("existing destination data\n")
        (root / "not-directory").write_text("regular file\n")
        self.engine = root / "engine/marker engine.sh"
        self.engine.write_bytes((FIXTURES / "engine-marker.sh").read_bytes())
        self.engine.chmod(0o755)
        (root / "engine/not-executable").write_text("not executable\n")
        (root / "engine/not-executable").chmod(0o644)
        for name, target in {
            "src-alias": root / "src", "dst-alias": root / "dst",
            "child-alias": root / "src/child", "dangling": root / "missing",
            "loop": root / "loop", "root-alias": Path("/"),
            "src/inner-dangling": root / "missing", "src/inner-root": Path("/"),
            "engine/dangling": root / "missing-engine",
        }.items():
            (root / name).symlink_to(target, target_is_directory=True)

    def task(self, profile="recursive"):
        task = json.loads((FIXTURES / (profile + ".json")).read_text())
        task["engineExecutable"] = str(self.root / "missing-engine")
        task["source"]["raw"] = str(self.root / "src")
        task["destination"]["raw"] = str(self.root / "dst")
        return task


def cases_for(fixture):
    cases = []
    base = fixture.task()

    def good(name, task=None, profile="recursive", action=None):
        cases.append({"id": name, "plans": ["PLAN-01", "PLAN-02", "PLAN-04", "PLAN-05"],
                      "task": copy.deepcopy(base if task is None else task),
                      "profile": profile, "action": action})

    def bad(name, task=None, code="invalid_task", field=None, raw=None, **extra):
        item = {"id": name, "plans": ["PLAN-01", "PLAN-06"],
                "task": copy.deepcopy(base if task is None else task),
                "error": code, "field": field}
        if raw is not None:
            item["raw"] = raw
        item.update(extra)
        cases.append(item)

    good("recursive")
    good("archive", fixture.task("archive"), "archive")
    for name, source_suffix, destination_suffix in (
        ("source-slash", "/", ""), ("destination-slash", "", "/"),
        ("both-slashes", "/", "/"),
    ):
        task = copy.deepcopy(base)
        task["source"]["raw"] += source_suffix
        task["destination"]["raw"] += destination_suffix
        good(name, task)
    for name, source, destination in (
        ("unicode-space", fixture.root / "中文 空格", fixture.root / "dst"),
        ("shell-literals-and-colon", fixture.shell, fixture.root / "dst"),
        ("component-prefix-siblings", fixture.root / "src", fixture.root / "src2"),
        ("independent-symlink-aliases", fixture.root / "src-alias", fixture.root / "dst-alias"),
    ):
        task = copy.deepcopy(base)
        task["source"]["raw"], task["destination"]["raw"] = str(source), str(destination)
        good(name, task)
    task = copy.deepcopy(base)
    task["source"]["raw"] = str(fixture.root / "src") + "/../src/"
    good("raw-dot-components-preserved", task)
    for name, engine in (
        ("engine-marker-never-executed", str(fixture.engine)),
        ("engine-nonexecutable-not-inspected", str(fixture.root / "engine/not-executable")),
        ("engine-directory-not-inspected", str(fixture.root / "engine")),
        ("engine-dangling-not-inspected", str(fixture.root / "engine/dangling")),
        ("engine-root-only-expression-check", "/"),
        ("engine-4096-byte-boundary", "/" + "e" * 4095),
    ):
        task = copy.deepcopy(base)
        task["engineExecutable"] = engine
        good(name, task)
    task = copy.deepcopy(base)
    task.update(taskId="é" * 64, name="中" * 85 + "x", revision=2**64 - 1)
    good("utf8-field-and-u64-boundaries", task)
    good("deterministic-repeat-first")
    good("deterministic-repeat-second")

    for field in REQUIRED_FIELDS:
        task = copy.deepcopy(base)
        del task[field]
        bad("missing-" + field, task)
        task = copy.deepcopy(base)
        task[field] = None
        bad("null-" + field, task)
    task = copy.deepcopy(base)
    task["unknown"] = True
    bad("unknown-task-field", task)
    task = copy.deepcopy(base)
    task["source"]["extra"] = "unexpected"
    bad("unknown-localpath-field", task)
    task = copy.deepcopy(base)
    task["options"][0]["extra"] = True
    bad("unknown-option-field", task)
    raw = encode(base)
    for name, before, after in (
        ("duplicate-task-key", b'"taskId":', b'"taskId":"first","taskId":'),
        ("duplicate-schema-key", b'"schemaVersion":', b'"schemaVersion":1,"schemaVersion":'),
        ("duplicate-localpath-key", b'"source":{"raw":', b'"source":{"raw":"first","raw":'),
        ("duplicate-option-tag", b'"options":[{"kind":', b'"options":[{"kind":"recursive","kind":'),
    ):
        assert before in raw
        bad(name, raw=raw.replace(before, after, 1))
    for value in (0, 2):
        task = copy.deepcopy(base)
        task["schemaVersion"] = value
        bad("unsupported-schema-" + str(value), task, "unsupported_schema", "schemaVersion")
    for name, field, value in (
        ("schema-overflow", "schemaVersion", 2**32),
        ("schema-wrong-type", "schemaVersion", "1"),
        ("schema-negative", "schemaVersion", -1),
        ("revision-zero", "revision", 0),
        ("revision-overflow", "revision", 2**64),
        ("revision-float", "revision", 1.5),
        ("wrong-kind", "kind", "rsync.task.other"),
        ("task-id-blank", "taskId", " \t\n"),
        ("task-id-nul", "taskId", "qa\x00id"),
        ("task-id-129-bytes", "taskId", "é" * 64 + "x"),
        ("name-blank", "name", "\u3000 \t"),
        ("name-nul", "name", "qa\x00name"),
        ("name-257-bytes", "name", "中" * 85 + "xx"),
    ):
        task = copy.deepcopy(base)
        task[field] = value
        bad(name, task)
    for name, options in (
        ("empty-options", []), ("duplicate-options", [{"kind": "recursive"}] * 2),
        ("two-options", [{"kind": "recursive"}, {"kind": "archive"}]),
        ("unknown-option", [{"kind": "delete"}]),
        ("object-recursive-option-tag", [{"kind": {"recursive": None}}]),
        ("object-archive-option-tag", [{"kind": {"archive": None}}]),
        ("bandwidth-not-in-scope", [{"kind": "bandwidth_limit_ki_b", "value": 1}]),
        ("string-option", ["--recursive"]),
    ):
        task = copy.deepcopy(base)
        task["options"] = options
        bad(name, task)

    for field in ("source", "destination", "engineExecutable"):
        for name, value in (("empty", ""), ("relative", "relative/path"),
                            ("nul", "/path\x00name"), ("4097-bytes", "/" + "x" * 4096)):
            task = copy.deepcopy(base)
            if field == "engineExecutable":
                task[field] = value
                error_field = field
            else:
                task[field]["raw"] = value
                error_field = field + ".raw"
            bad(field + "-" + name, task, "invalid_path", error_field)
    for name, value in (("remote-host", "host:path"), ("remote-url", "rsync://host/path"),
                        ("tilde-not-expanded", "~/source")):
        task = copy.deepcopy(base)
        task["source"]["raw"] = value
        bad(name, task, "invalid_path", "source.raw")

    for name, source, destination in (
        ("same-directory", "src", "src"), ("nested-destination", "src", "src/child"),
        ("nested-source", "src/child", "src"), ("same-symlink", "src", "src-alias"),
        ("nested-symlink-destination", "src", "child-alias"),
        ("nested-symlink-source", "child-alias", "src"),
        ("same-via-dot-components", "src", "src2/../src"),
    ):
        task = copy.deepcopy(base)
        task["source"]["raw"] = str(fixture.root / source)
        task["destination"]["raw"] = str(fixture.root / destination)
        bad(name, task, "unsafe_path_relation", "destination.raw", plans=["PLAN-03", "PLAN-06"])
    for field in ("source", "destination"):
        for name, value in (("root", "/"), ("root-alias", str(fixture.root / "root-alias")),
                            ("missing", str(fixture.root / "missing")),
                            ("file", str(fixture.root / "not-directory")),
                            ("not-directory-parent", str(fixture.root / "not-directory/child")),
                            ("dangling", str(fixture.root / "dangling"))):
            task = copy.deepcopy(base)
            task[field]["raw"] = value
            bad(field + "-" + name, task, "invalid_path", field + ".raw", plans=["PLAN-03", "PLAN-06"])
    task = copy.deepcopy(base)
    task["source"]["raw"] = str(fixture.root / "loop")
    bad("symlink-loop-io-error", task, "path_io", "source.raw", require_os_code=True)

    for name, raw in (("json-truncated", raw[:-1]), ("json-empty", b""),
                      ("json-syntax", raw.replace(b'"revision":1', b'"revision":', 1)),
                      ("json-trailing", raw + b"garbage")):
        bad(name, raw=raw, code="invalid_json")
    padded = encode(base)
    good("json-exact-64k")
    cases[-1]["raw"] = padded + b" " * (MAX_JSON - len(padded))
    bad("json-over-64k", raw=padded + b" " * (MAX_JSON + 1 - len(padded)), field="json")
    text = padded.decode()
    bad("json-byte-not-character-limit", raw=(text + " " * (MAX_JSON - 1 - len(text))).encode(), field="json")
    bad("task-file-invalid-utf8", raw=b"\xff\xfe", code="invalid_json", field="taskFile")
    bad("task-file-missing", code="invalid_path", field="taskFile", arguments=["plan", "@MISSING_FILE@"])
    for name, arguments in (("cli-unknown", ["run"]), ("cli-missing-file", ["plan"]),
                            ("cli-extra-argument", ["plan", "@TASK_FILE@", "extra"])):
        bad(name, field="arguments", arguments=arguments)
    cases.append({"id": "setup-health-compatible", "plans": ["PLAN-06", "PLAN-08"], "health": True})

    for field in ("source", "destination"):
        task = copy.deepcopy(base)
        task[field]["raw"] = str(fixture.root / "protected/inner")
        bad(field + "-access-denied", task, "path_access_denied", field + ".raw",
            permission=True, require_os_code=True, plans=["PLAN-03", "PLAN-06"])
    task = copy.deepcopy(base)
    task["source"]["raw"] = str(fixture.root / "transient-source")
    good("directory-present-before-removal", task, action="create-transient")
    bad("directory-removed-after-plan", task, "invalid_path", "source.raw", action="remove-transient")
    assert len({case["id"] for case in cases}) == len(cases)
    return cases


def oracle(task, profile, expectations):
    # Explicit fixture tokens, never obtained from or computed by the Rust planner.
    replacements = {"@SOURCE@": task["source"]["raw"], "@DESTINATION@": task["destination"]["raw"]}
    return {
        "contractVersion": "planner.v1", "taskSnapshot": task,
        "executable": task["engineExecutable"],
        "runArgv": [replacements.get(token, token) for token in expectations[profile]["runArgv"]],
        "dryRunArgv": [replacements.get(token, token) for token in expectations[profile]["dryRunArgv"]],
        "pathCheck": "checked_at_planning", "engineCheck": "not_performed",
    }


def run_case(case, fixture, probe, inputs, expected_version, expectations):
    result = {"caseId": case["id"], "plans": case["plans"]}
    protected = fixture.root / "protected"
    permission = case.get("permission", False)
    try:
        if case.get("action") == "create-transient":
            (fixture.root / "transient-source").mkdir()
        elif case.get("action") == "remove-transient":
            (fixture.root / "transient-source").rmdir()
        if permission:
            protected.chmod(0)
            try:
                (protected / "inner").stat()
            except PermissionError:
                pass
            else:
                result.update(status="UNVERIFIED", reason="Current identity bypasses directory permissions; precondition not met.")
                return result
        raw = case.get("raw", encode(case.get("task", {})))
        input_path = inputs / (case["id"] + ".json")
        input_path.write_bytes(raw)
        input_before = (digest(raw), input_path.stat().st_mtime_ns, input_path.stat().st_mode)
        replacements = {"@TASK_FILE@": str(input_path), "@MISSING_FILE@": str(inputs / "missing-task.json")}
        arguments = [] if case.get("health") else case.get("arguments", ["plan", "@TASK_FILE@"])
        argv = [str(probe), *[replacements.get(value, value) for value in arguments]]
        before = tree_manifest(fixture.root)
        started = time.monotonic()
        completed = subprocess.run(argv, cwd=fixture.root, capture_output=True, timeout=10)
        after = tree_manifest(fixture.root)
        problems = []
        if before != after:
            problems.append("fixture entries/content/modes/mtime changed")
            result["changedEntries"] = [key for key in before.keys() | after.keys() if before.get(key) != after.get(key)]
        input_after = (digest(input_path.read_bytes()), input_path.stat().st_mtime_ns, input_path.stat().st_mode)
        if input_before != input_after:
            problems.append("task input file changed")
        marker = fixture.engine.with_name(fixture.engine.name + ".executed")
        if marker.exists():
            problems.append("engine marker script executed")
        result.update(argv=argv, inputFile=str(input_path), inputSha256=digest(raw),
                      exitCode=completed.returncode, elapsedSeconds=round(time.monotonic()-started, 6),
                      stdout=completed.stdout.decode("utf-8", errors="backslashreplace"),
                      stderr=completed.stderr.decode("utf-8", errors="backslashreplace"),
                      fixtureBeforeSha256=digest(encode(before)), fixtureAfterSha256=digest(encode(after)),
                      engineMarkerPresent=marker.exists())
        if case.get("health"):
            expected = f"ctsf-kernel version={expected_version} contract=setup.v1 health=ready\n".encode()
            if completed.returncode != 0 or completed.stdout != expected or completed.stderr:
                problems.append("legacy health stdout/stderr/exit mismatch")
        elif "error" in case:
            if completed.returncode != 2 or completed.stdout:
                problems.append("expected exit 2 and empty stdout")
            if not completed.stderr.endswith(b"\n") or completed.stderr.count(b"\n") != 1:
                problems.append("expected one stderr JSON line with final LF")
            try:
                error = strict_json(completed.stderr)
                if not isinstance(error, dict) or error.get("code") != case["error"]:
                    problems.append("PlanningError code mismatch")
                elif case["field"] is not None and error.get("field") != case["field"]:
                    problems.append("PlanningError field mismatch")
                if not isinstance(error.get("message"), str) or not error["message"]:
                    problems.append("PlanningError message missing")
                if error.get("field") is not None and not isinstance(error["field"], str):
                    problems.append("PlanningError field must be string/null")
                if error.get("osCode") is not None and type(error["osCode"]) is not int:
                    problems.append("PlanningError osCode must be integer/null")
                if case.get("require_os_code") and type(error.get("osCode")) is not int:
                    problems.append("OS error code was not preserved")
            except (ValueError, AttributeError, TypeError, KeyError):
                problems.append("stderr is not a valid PlanningError JSON")
            result["expectedError"] = {"code": case["error"], "field": case["field"] or "not constrained for serde structural errors"}
        else:
            if completed.returncode != 0 or completed.stderr:
                problems.append("expected exit 0 and empty stderr")
            if not completed.stdout.endswith(b"\n") or completed.stdout.count(b"\n") != 1:
                problems.append("expected one stdout JSON line with final LF")
            expected = oracle(case["task"], case["profile"], expectations)
            result["expectedPlan"] = expected
            try:
                if strict_json(completed.stdout) != expected:
                    problems.append("plan differs from handwritten oracle")
            except (ValueError, TypeError):
                problems.append("stdout is not valid unique-key UTF-8 plan JSON")
        result.update(status="FAIL" if problems else "PASS", problems=problems)
        return result
    except (OSError, subprocess.SubprocessError) as error:
        result.update(status="ERROR", reason=str(error))
        return result
    finally:
        if permission:
            protected.chmod(0o755)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--probe", type=Path, help="absolute path to an already-built real kernel-probe")
    parser.add_argument("--report", type=Path, help="absolute JSON report path; adjacent unique input artifacts are retained")
    parser.add_argument("--expected-version", default="0.1.0")
    parser.add_argument("--source-id", default="unspecified; see probe SHA-256")
    parser.add_argument("--check-fixtures", action="store_true", help="prepare/check inputs only; do not invoke a probe")
    args = parser.parse_args()
    expectations = json.loads((FIXTURES / "expected-argv.json").read_text())
    with tempfile.TemporaryDirectory(prefix="ctsf-planner-data-") as temporary:
        fixture = FixtureTree(Path(temporary).resolve())
        cases = cases_for(fixture)
        if args.check_fixtures:
            print(json.dumps({"status": "PREPARED_ONLY", "probeExecuted": False, "cases": len(cases)}))
            return 0
        if args.probe is None or not args.probe.is_absolute():
            parser.error("--probe must name an absolute, already-built real executable")
        probe = args.probe.resolve()
        if not probe.is_file() or not os.access(probe, os.X_OK):
            parser.error("probe is missing or not executable; no acceptance tests ran")
        if args.report is not None and not args.report.is_absolute():
            parser.error("--report must be absolute")
        report_path = args.report or Path(tempfile.mkdtemp(prefix="ctsf-planner-evidence-")) / "report.json"
        report_path.parent.mkdir(parents=True, exist_ok=True)
        inputs = Path(tempfile.mkdtemp(prefix=report_path.stem + "-inputs-", dir=report_path.parent))
        probe_hash = digest(probe.read_bytes())
        results = [run_case(case, fixture, probe, inputs, args.expected_version, expectations) for case in cases]
        counts = {status: sum(result["status"] == status for result in results)
                  for status in ("PASS", "FAIL", "ERROR", "UNVERIFIED")}
        changed = digest(probe.read_bytes()) != probe_hash
        status = "FAIL" if counts["FAIL"] or counts["ERROR"] or changed else (
            "PASS_WITH_LIMITATIONS" if counts["UNVERIFIED"] else "PASS")
        report = {
            "contract": "planner.v1", "status": status, "counts": counts,
            "probe": str(probe), "probeSha256": probe_hash, "probeChangedDuringRun": changed,
            "sourceId": args.source_id, "expectedKernelVersion": args.expected_version,
            "environment": {"platform": platform.platform(), "uname": list(platform.uname()),
                            "python": sys.version, "effectiveUid": os.geteuid()},
            "harnessSha256": digest(Path(__file__).read_bytes()),
            "fixtureSha256": {path.name: digest(path.read_bytes()) for path in sorted(FIXTURES.iterdir()) if path.is_file()},
            "blackBoxLimitations": [
                "Rust typed DTO direct-call validation and in-process ownership/serde APIs require developer tests.",
                "Marker/integrity checks observe these invocations; they are not general OS process tracing.",
                "PLAN-07 build/toolchain and PLAN-08 workspace/FFI checks need separate execution evidence.",
                "Planning neither runs rsync nor proves Dry Run/transfer/cancel behavior or native feature acceptance.",
            ],
            "results": results,
        }
        report_path.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
        print(json.dumps({"status": status, "counts": counts, "report": str(report_path)}, ensure_ascii=False))
        for result in results:
            if result["status"] in ("FAIL", "ERROR"):
                print(result["caseId"] + ": " + str(result.get("problems", result.get("reason"))), file=sys.stderr)
        return 1 if status == "FAIL" else 0


if __name__ == "__main__":
    raise SystemExit(main())
