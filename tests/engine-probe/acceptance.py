#!/usr/bin/env python3
"""Independent engine-probe.v1 acceptance; Python standard library, no builds."""

import argparse
import ast
import copy
import datetime
import errno
import hashlib
import json
import os
from pathlib import Path
import platform
import selectors
import signal
import stat
import subprocess
import sys
import tempfile
import time
import uuid


ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / "fixtures/engine-probe"
TOKENS = ["--recursive", "--archive", "--dry-run", "--"]
ENVIRONMENT = {"PATH": "/usr/bin:/bin", "LC_ALL": "C", "LANG": "C", "TZ": "UTC"}
WATCHDOG = 8.0
CLI_LIMIT = 512 * 1024


def require(condition, detail):
    if not condition:
        raise AssertionError(detail)


def load_json(data):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError("duplicate JSON field: " + key)
            result[key] = value
        return result
    return json.loads(data.decode("utf-8"), object_pairs_hook=unique)


def save(path, value):
    path.write_text(json.dumps(value, ensure_ascii=True, indent=2) + "\n", encoding="utf-8")


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def prepared():
    data = load_json((FIXTURES / "observations.json").read_bytes())
    ast.parse((FIXTURES / "helper.py").read_bytes())
    require(len(data["supported"]) == len(data["mixed"]) == len(data["missing"]) == 4,
            "handwritten capability oracles must contain exactly four entries")
    require(data["openrsyncVersion"].endswith("2.6.9 compatible\n"), "compatibility fixture")
    require(data["fullHelp"].count("\n") == 5, "fixed evidence line numbers changed")
    return data


def captured(command, directory, environment=None):
    """Bound both outer CLI streams too; never wait forever for inherited pipes."""
    started = time.monotonic()
    proc = subprocess.Popen(command, cwd=str(directory), env=environment, stdin=subprocess.DEVNULL,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
    selector = selectors.DefaultSelector()
    output = {"stdout": bytearray(), "stderr": bytearray()}
    counts = {"stdout": 0, "stderr": 0}
    for name, stream in [("stdout", proc.stdout), ("stderr", proc.stderr)]:
        os.set_blocking(stream.fileno(), False)
        selector.register(stream, selectors.EVENT_READ, name)
    timeout = False
    exceeded = False
    killed_at = None
    killed_probe = False
    while selector.get_map() or proc.poll() is None:
        now = time.monotonic()
        if killed_at is None and (now - started >= WATCHDOG or exceeded):
            timeout = now - started >= WATCHDOG
            # Own Popen child only. Helpers have separate groups owned by production.
            if proc.poll() is None:
                proc.kill()
                killed_probe = True
            killed_at = now
        if killed_at is not None and now - killed_at >= 0.5:
            break
        for key, _ in selector.select(0.02):
            for _ in range(4):
                try:
                    block = os.read(key.fd, 4096)
                except BlockingIOError:
                    break
                except InterruptedError:
                    continue
                if not block:
                    selector.unregister(key.fileobj)
                    break
                name = key.data
                counts[name] += len(block)
                output[name].extend(block[:max(0, CLI_LIMIT - len(output[name]))])
                if counts[name] > CLI_LIMIT:
                    exceeded = True
    selector.close()
    proc.stdout.close()
    proc.stderr.close()
    if proc.poll() is None:
        proc.kill()
    proc.wait(timeout=1)
    return {"argv": command, "exitCode": proc.returncode,
            "elapsedSeconds": time.monotonic() - started,
            "watchdogTriggered": timeout, "outerOutputLimitExceeded": exceeded,
            "qaKilledProbe": killed_probe, "observedBytes": counts,
            "stdout": bytes(output["stdout"]), "stderr": bytes(output["stderr"])}


def cases(data):
    base = {"version": {"stdout": data["rsyncVersion"]}, "help": {"stdout": data["fullHelp"]}}
    result = []

    def add(name, edits=None, **expect):
        scenario = copy.deepcopy(base)
        for step, changes in (edits or {}).items():
            scenario[step].update(changes)
        item = {"id": name, "scenario": scenario, "kind": "rsync", "version": "3.4.1",
                "compatibility": None, "protocol": 32, "capabilities": data["supported"],
                "commands": 2}
        item.update(expect)
        result.append(item)

    unknown = [["unknown", "unknown_implementation"]] * 4
    unusable = [["unknown", "unusable_help"]] * 4
    add("upstream-exact-argv-environment-and-evidence", evidenceLines=[2, 3, 4, 5], trailingSpace=True)
    add("openrsync-compatibility-is-not-implementation-version", {
        "version": {"stdout": "", "stderr": data["openrsyncVersion"]},
        "help": {"stdout": "usage: rsync [-ran] source destination\n"}},
        kind="openrsync", version=None, compatibility="2.6.9", protocol=29,
        capabilities=data["missing"])
    add("openrsync-explicit-release", {"version": {"stdout": data["openrsyncRelease"]}},
        kind="openrsync", version="0.5.0", protocol=27)
    add("unknown-banner-does-not-borrow-known-help", {"version": {"stdout": "mystery engine 9.9\n"}},
        kind="unknown", version=None, protocol=None, capabilities=unknown)
    add("conflicting-identity-across-streams", {"version": {"stderr": data["openrsyncRelease"]}},
        kind="unknown", version=None, protocol=None, capabilities=unknown)
    add("explicit-negative-and-missing-capabilities", {"help": {"stdout": data["mixedHelp"]}},
        capabilities=data["mixed"])
    add("short-usage-prefixes-and-prose-are-not-evidence", {"help": {"stdout": data["deceptiveHelp"]}},
        capabilities=data["missing"])
    add("bare-separator-must-have-own-option-column", {"help": {"stdout": data["combinedSeparatorHelp"]}},
        capabilities=[["supported", "help_entry"], ["unknown", "missing_evidence"],
                      ["unknown", "missing_evidence"], ["unknown", "missing_evidence"]])
    add("conflicting-and-qualified-negative-help", {"help": {
        "stdout": data["conflictingHelpOut"], "stderr": data["conflictingHelpErr"]}},
        capabilities=[["unknown", "conflicting_evidence"], ["unknown", "conflicting_evidence"],
                      ["unknown", None], ["unknown", "missing_evidence"]])
    add("invalid-version-utf8-retains-raw-bytes", {"version": {
        "stdoutBytes": list(data["rsyncVersion"].encode()) + [255]}},
        kind="unknown", version=None, protocol=None, capabilities=unknown)
    add("invalid-help-utf8-cannot-support-options", {"help": {
        "stderrBytes": [0, 255, 254]}}, capabilities=unusable)
    exact_version = data["rsyncVersion"].encode() + b"x" * (16384 - len(data["rsyncVersion"].encode()))
    exact_help = data["fullHelp"].encode() + b"x" * (16384 - len(data["fullHelp"].encode()))
    add("exact-per-stream-budget-and-no-final-newline", {
        "version": {"stdoutBytes": list(exact_version), "stderr": "y" * 16384},
        "help": {"stdoutBytes": list(exact_help), "stderr": "z" * 16384}})
    add("stdout-one-byte-over-budget-stops-before-help", {
        "version": {"stdout": "x" * 16385}}, error="output_limit_exceeded", step="version",
        kind="unknown", version=None, protocol=None, capabilities=unknown, commands=1, overflow=True)
    add("both-stream-flood-drains-after-limit", {"help": {
        "dualFlood": True, "stdout": "x" * 262144, "stderr": "y" * 262144}},
        error="output_limit_exceeded", step="help", capabilities=unusable, overflow=True, flood=True)
    add("nonzero-version-still-observes-help", {"version": {"exit": 7}},
        error="non_zero_exit", step="version", kind="unknown", version=None, protocol=None,
        capabilities=unknown, exitCodes=[7, 0])
    add("nonzero-help-preserves-version-facts", {"help": {"exit": 8}},
        error="non_zero_exit", step="help", capabilities=unusable, exitCodes=[0, 8])
    add("signalled-version-is-not-success", {"version": {"signal": "SIGTERM"}},
        error="non_zero_exit", step="version", kind="unknown", version=None, protocol=None,
        capabilities=unknown, signalStep="version")
    add("silent-version-deadline-and-no-second-spawn", {"version": {"delay": 20}},
        error="deadline_exceeded", step="version", kind="unknown", version=None, protocol=None,
        capabilities=unknown, commands=1, deadline=True)
    add("ignored-term-requires-kill", {"version": {"ignoreTerm": True, "delay": 20}},
        error="deadline_exceeded", step="version", kind="unknown", version=None, protocol=None,
        capabilities=unknown, commands=1, deadline=True, killed=True)
    add("help-shares-version-deadline", {"version": {"delay": 1.6}, "help": {"delay": 20}},
        error="deadline_exceeded", step="help", capabilities=unusable, deadline=True, shared=True)
    add("normal-cleanup-crosses-deadline-without-fake-help", {"version": {"delay": 2.8}},
        error="deadline_exceeded", step="help", commands=1, capabilities=unusable, cleanupDeadline=True)
    add("leader-exits-descendant-retains-pipes", {"version": {"descendant": True}},
        error="deadline_exceeded", step="version", kind="unknown", version=None, protocol=None,
        capabilities=unknown, commands=1, deadline=True, descendant=True,
        allowCleanupUnconfirmed=True)
    add("closed-pipes-are-not-process-exit", {"version": {"closePipes": True, "delay": 20}},
        error="non_zero_exit", step="version", kind="unknown", version=None, protocol=None,
        capabilities=unknown, signalStep="version")
    add("engine-change-is-not-retried", {"version": {"mutateExecutable": True}},
        error="engine_changed", step="help", commands=1, capabilities=unusable)
    add("top-level-symlink-preserves-requested-path", symlink=True)
    for name, path_kind, code in [
        ("missing-engine", "missing", "engine_not_found"),
        ("directory-is-not-engine", "directory", "invalid_engine_path"),
        ("nonexecutable-engine", "nonexecutable", "permission_denied"),
        ("relative-engine-is-not-path-searched", "relative", "invalid_engine_path"),
        ("empty-engine-path", "empty", "invalid_engine_path"),
        ("overlong-engine-path", "overlong", "invalid_engine_path"),
    ]:
        add(name, pathKind=path_kind, error=code,
            step="version" if path_kind == "nonexecutable" else None, commands=0, noChild=True)
    return result


def fields(value, expected, label):
    require(isinstance(value, dict) and set(value) == set(expected.split()), label + " fields")


def stream_bytes(stream):
    fields(stream, "bytes observedBytes truncated utf8Valid eof", "captured stream")
    require(isinstance(stream["bytes"], list) and all(type(n) is int and 0 <= n <= 255 for n in stream["bytes"]),
            "raw output must be a u8 array")
    raw = bytes(stream["bytes"])
    require(len(raw) <= 16384 and type(stream["observedBytes"]) is int
            and stream["observedBytes"] >= len(raw), "capture/observed byte bounds")
    for key in ("truncated", "utf8Valid", "eof"):
        require(type(stream[key]) is bool, "stream " + key + " must be boolean")
    require(stream["truncated"] == (stream["observedBytes"] > len(raw)), "truncation must describe discarded bytes")
    try:
        raw.decode("utf-8")
        valid = True
    except UnicodeDecodeError:
        valid = False
    require(stream["utf8Valid"] == valid, "UTF-8 flag does not match retained bytes")
    return raw


def evidence_refs(refs, commands):
    require(isinstance(refs, list) and len(refs) <= 2, "evidence refs bound")
    lines = []
    for ref in refs:
        fields(ref, "step stream line", "evidence ref")
        require(ref["stream"] in ("stdout", "stderr") and type(ref["line"]) is int and ref["line"] >= 1,
                "evidence stream/line")
        matches = [c for c in commands if c["step"] == ref["step"]]
        require(len(matches) == 1, "evidence step absent")
        command = matches[0]
        require(command["exitCode"] == 0 and not command["timedOut"]
                and command["cleanup"]["childReaped"] and command["cleanup"]["group"] == "gone"
                and all(command[s]["eof"] and command[s]["utf8Valid"] and not command[s]["truncated"]
                        for s in ("stdout", "stderr")), "evidence from unusable command")
        raw_lines = bytes(command[ref["stream"]]["bytes"]).split(b"\n")
        require(ref["line"] <= len(raw_lines), "evidence line outside original stream")
        lines.append(raw_lines[ref["line"] - 1])
    return lines


def envelope(result):
    require(not result["watchdogTriggered"], "8-second outer watchdog fired; QA cleanup is not production success")
    require(not result["outerOutputLimitExceeded"], "outer CLI output exceeds independent 512 KiB bound")
    require(result["exitCode"] in (0, 2), "unexpected probe exit " + str(result["exitCode"]))
    good = result["exitCode"] == 0
    raw = result["stdout" if good else "stderr"]
    require(result["stderr" if good else "stdout"] == b"", "wrong CLI output stream")
    require(raw.endswith(b"\n") and raw.count(b"\n") == 1, "CLI must emit one compact JSON envelope plus newline")
    value = load_json(raw)
    fields(value, "contractVersion status report error", "CLI envelope")
    require(value["contractVersion"] == "engine-probe.v1", "CLI contract")
    require(value["status"] == ("ok" if good else "error"), "status/exit mismatch")
    if good:
        require(value["error"] is None and value["report"] is not None, "success envelope")
    else:
        fields(value["error"], "code step message osCode", "CLI error")
        error = value["error"]
        require(isinstance(error["message"], str) and len(error["message"].encode()) <= 2048, "message bound")
        require(error["osCode"] is None or type(error["osCode"]) is int, "optional errno")
    return value


def validate_report(report, requested, resolved):
    fields(report, "schemaVersion contractVersion requestedExecutable resolvedExecutable identity capabilities commands elapsedMs",
           "report")
    require(report["schemaVersion"] == 1 and report["contractVersion"] == "engine-probe.v1", "report contract")
    require(report["requestedExecutable"].encode() == requested.encode(), "requested path bytes changed")
    require(report["resolvedExecutable"].encode() == resolved.encode(), "wrong resolved executable")
    require(type(report["elapsedMs"]) is int and report["elapsedMs"] >= 0, "report elapsed")
    commands = report["commands"]
    require(isinstance(commands, list) and len(commands) <= 2, "command bound")
    for index, command in enumerate(commands):
        fields(command, "step argv pid exitCode termSignal elapsedMs timedOut stdout stderr cleanup", "command")
        step = ["version", "help"][index]
        require(command["step"] == step and command["argv"] == ["--" + step], "step/order/argv")
        require(type(command["pid"]) is int and command["pid"] > 1, "real PID required")
        require(type(command["elapsedMs"]) is int and command["elapsedMs"] >= 0, "command elapsed")
        require(type(command["timedOut"]) is bool, "timedOut type")
        require(command["exitCode"] is None or type(command["exitCode"]) is int, "exitCode type")
        require(command["termSignal"] is None or type(command["termSignal"]) is int, "termSignal type")
        require(not (command["exitCode"] is not None and command["termSignal"] is not None), "exit and signal conflict")
        for stream in ("stdout", "stderr"):
            stream_bytes(command[stream])
        cleanup = command["cleanup"]
        fields(cleanup, "childReaped group signals", "cleanup")
        require(type(cleanup["childReaped"]) is bool and cleanup["group"] in ("gone", "unconfirmed"), "cleanup types")
        require(isinstance(cleanup["signals"], list) and len(cleanup["signals"]) <= 2, "signal bound")
        signals = []
        for attempt in cleanup["signals"]:
            fields(attempt, "signal sent osCode", "signal attempt")
            require(type(attempt["sent"]) is bool and (attempt["osCode"] is None or type(attempt["osCode"]) is int),
                    "signal observation types")
            signals.append(attempt["signal"])
        require(signals in ([], ["term"], ["kill"], ["term", "kill"]), "signal order/duplicates")
    identity = report["identity"]
    fields(identity, "kind implementationVersion compatibilityVersion protocolVersion evidence", "identity")
    require(identity["kind"] in ("rsync", "openrsync", "unknown"), "identity kind")
    evidence_refs(identity["evidence"], commands)
    caps = report["capabilities"]
    require(isinstance(caps, list) and [cap["token"] for cap in caps] == TOKENS, "ordered four capabilities")
    for cap in caps:
        fields(cap, "token status reason evidence", "capability")
        require(cap["status"] in ("supported", "unsupported", "unknown"), "capability status")
        require(cap["reason"] in ("help_entry", "explicit_unsupported", "missing_evidence", "unusable_help",
                                  "unknown_implementation", "conflicting_evidence"), "capability reason")
        lines = evidence_refs(cap["evidence"], commands)
        if cap["status"] != "unknown":
            require(lines and any(cap["token"].encode() in line for line in lines), "capability lacks actual line evidence")
    return commands


def process_state(pid):
    """Observe only a fixture-recorded PID; never signal numeric recorded IDs."""
    proc = Path("/proc") / str(pid) / "stat"
    if Path("/proc/self/stat").exists():
        try:
            raw = proc.read_text()
            return {"state": raw.rsplit(")", 1)[1].split()[0], "source": "proc"}
        except FileNotFoundError:
            return {"state": "gone", "source": "proc"}
    try:
        query = subprocess.run(["/bin/ps", "-p", str(pid), "-o", "stat="],
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=1)
        state = query.stdout.decode("ascii", "replace").strip()
        if query.returncode == 0 and state:
            return {"state": state[0], "source": "ps"}
        if query.returncode == 1 and not query.stderr:
            return {"state": "gone", "source": "ps"}
    except (OSError, subprocess.SubprocessError):
        # Sandbox/process-observation failure must not skip fixture self-cleanup.
        pass
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return {"state": "gone", "source": "kill0"}
    except PermissionError:
        pass
    return {"state": "unverified", "source": "process observation unavailable"}


def fixture_events(directory):
    events = []
    for path in sorted(directory.glob("*-event.json")) + sorted(directory.glob("descendant-*.json")):
        event = load_json(path.read_bytes())
        event["file"] = path.name
        events.append(event)
    return events


def cleanup_fixture(directory, result):
    events = fixture_events(directory)
    observations = [{"pid": e["pid"], "role": e["role"], **process_state(e["pid"])} for e in events]
    alive = [p for p in observations if p["state"] not in ("gone", "Z", "unverified")]
    needed = result["qaKilledProbe"] or result["watchdogTriggered"] or result["outerOutputLimitExceeded"] or bool(alive)
    # Each helper watches its private directory and exits itself. No stale PGID signals.
    # Write even with no event: a broken owner could return before Python records readiness.
    (directory / "qa-stop").touch()
    if needed or any(p["state"] == "unverified" for p in observations):
        until = time.monotonic() + 1
        while time.monotonic() < until:
            if all(process_state(e["pid"])["state"] in ("gone", "Z", "unverified") for e in events):
                break
            time.sleep(0.02)
    return {"requested": needed, "stopFileWritten": (directory / "qa-stop").exists(),
            "method": "private fixture self-stop; own Popen kill on watchdog only",
            "before": observations,
            "after": [{"pid": e["pid"], **process_state(e["pid"])} for e in events]}


def make_fixture(directory, item):
    directory.mkdir()
    executable = directory / "引擎 空格 : $HOME ; [x] ' literal"
    if item.get("trailingSpace"):
        executable = executable.with_name(executable.name + " ")
    interpreter = str(Path(sys.executable).resolve())
    require(" " not in interpreter and "\n" not in interpreter and len(interpreter.encode()) < 240,
            "fixture needs an absolute Python interpreter usable in a Unix shebang")
    executable.write_bytes(("#!" + interpreter + "\n").encode() + (FIXTURES / "helper.py").read_bytes())
    executable.chmod(0o755)
    scenario = copy.deepcopy(item["scenario"])
    scenario["nonce"] = uuid.uuid4().hex
    save(directory / "scenario.json", scenario)
    for name in ("source", "destination"):
        (directory / name).mkdir()
        (directory / name / "payload").write_bytes(b"unchanged fixture data\x00\xff\n")
    requested = str(executable)
    if item.get("symlink"):
        requested = str(directory / "selected-alias")
        Path(requested).symlink_to(executable)
    path_kind = item.get("pathKind")
    if path_kind == "missing":
        requested = str(directory / "not-installed")
    elif path_kind == "directory":
        requested = str(directory)
    elif path_kind == "nonexecutable":
        executable.chmod(0o644)
    elif path_kind == "relative":
        requested = "relative-engine"
    elif path_kind == "empty":
        requested = ""
    elif path_kind == "overlong":
        requested = "/" + "x" * 4096
    return executable, requested, scenario


def integrity(directory):
    result = {}
    for name in ("source", "destination"):
        for path in [directory / name] + sorted((directory / name).rglob("*")):
            info = path.lstat()
            result[str(path.relative_to(directory))] = {
                "mode": stat.S_IMODE(info.st_mode), "mtimeNs": info.st_mtime_ns,
                "sha256": sha(path) if path.is_file() else None}
    return result


def check_fixture(item, value, result, directory, executable, requested, scenario, runtime_extra):
    error = value["error"]
    if item.get("error"):
        require(error is not None, "expected error " + item["error"])
        expected = item["error"]
        if item.get("allowCleanupUnconfirmed") and error["code"] == "cleanup_unconfirmed":
            expected = "cleanup_unconfirmed"
        require(error["code"] == expected, "error code: expected %s, got %s" % (expected, error["code"]))
        require(error["step"] == item["step"], "error step")
    else:
        require(error is None, "unexpected product error: " + str(error))
    report = value["report"]
    if item.get("noChild"):
        require(report is None or report["commands"] == [], "preflight must not invent a child")
        require(not fixture_events(directory), "invalid path executed helper")
        if report is not None:
            validate_report(report, requested, str(executable.resolve()))
        if item.get("pathKind") == "nonexecutable":
            require(error["osCode"] == errno.EACCES, "real spawn permission error must retain EACCES")
        return
    require(report is not None, "spawned command lost partial report")
    commands = validate_report(report, requested, str(executable.resolve()))
    require(len(commands) == item["commands"], "number of spawned commands")
    identity = report["identity"]
    for key, wanted in [("kind", item["kind"]), ("implementationVersion", item["version"]),
                        ("compatibilityVersion", item["compatibility"]), ("protocolVersion", item["protocol"])]:
        require(identity[key] == wanted, "identity " + key + ": " + repr(identity[key]))
    for cap, expected in zip(report["capabilities"], item["capabilities"]):
        require(cap["status"] == expected[0], "capability status " + cap["token"] + ": " + cap["status"])
        if expected[1] is not None:
            require(cap["reason"] == expected[1], "capability reason " + cap["token"] + ": " + cap["reason"])
    if item.get("evidenceLines"):
        require([c["evidence"] for c in report["capabilities"]] == [
            [{"step": "help", "stream": "stdout", "line": line}] for line in item["evidenceLines"]],
            "handwritten evidence line oracle")
    for index, command in enumerate(commands):
        event = load_json((directory / (command["step"] + "-event.json")).read_bytes())
        require(event["nonce"] == scenario["nonce"] and event["pid"] == command["pid"], "PID handshake")
        require(event["pgid"] == command["pid"] and event["pgid"] != os.getpgrp(), "dedicated child process group")
        require(event["argv"] == [str(executable.resolve()), "--" + command["step"]], "actual helper argv")
        expected_environment = dict(ENVIRONMENT)
        expected_environment.update(runtime_extra)
        require(event["cwd"] == "/" and event["environment"] == expected_environment and event["stdin"] == [],
                "actual helper environment/cwd/stdin: " + repr(event))
        cleanup = command["cleanup"]
        require(cleanup["childReaped"], "direct child not reaped")
        if not item.get("allowCleanupUnconfirmed"):
            require(cleanup["group"] == "gone" and command["stdout"]["eof"] and command["stderr"]["eof"],
                    "controlled fixture cleanup not confirmed")
        elif cleanup["group"] != "gone" or not all(command[s]["eof"] for s in ("stdout", "stderr")):
            require(error["code"] == "cleanup_unconfirmed", "cleanup error must take priority")
        spec = scenario[command["step"]]
        for stream in ("stdout", "stderr"):
            actual = bytes(command[stream]["bytes"])
            original = bytes(spec[stream + "Bytes"]) if stream + "Bytes" in spec else spec.get(stream, "").encode()
            if spec.get("dualFlood"):
                original = (stream + "-ready\n").encode() + original
            if item.get("deadline") or item.get("signalStep") == command["step"]:
                continue
            if spec.get("dualFlood") or item.get("overflow"):
                require(actual == original[:len(actual)], "raw overflow prefix changed")
            else:
                require(actual == original and command[stream]["observedBytes"] == len(original),
                        "raw stream bytes/count changed: " + command["step"] + "/" + stream)
        if not item.get("deadline") and not item.get("overflow") and item.get("signalStep") != command["step"]:
            expected_exit = item.get("exitCodes", [0, 0])[index]
            require(command["exitCode"] == expected_exit and command["termSignal"] is None, "actual exit status")
    if item.get("overflow"):
        last = commands[-1]
        require(any(last[s]["truncated"] and last[s]["observedBytes"] > 16384 for s in ("stdout", "stderr")),
                "output limit requires real discarded bytes")
        if item.get("flood"):
            require(all(last[s]["observedBytes"] > 0 for s in ("stdout", "stderr")), "both flood streams must be observed")
    if item.get("deadline"):
        require(commands[-1]["timedOut"] and 2.8 <= result["elapsedSeconds"] < 5.5, "shared deadline timing/fact")
    if item.get("shared"):
        require(commands[-1]["elapsedMs"] < 2300, "Help got a fresh execution deadline")
    if item.get("cleanupDeadline"):
        require(not commands[0]["timedOut"] and commands[0]["exitCode"] == 0
                and not (directory / "help-event.json").exists(),
                "normal EOF cleanup must not retroactively time out Version or invent Help")
    if item.get("killed"):
        last = commands[-1]
        require(last["termSignal"] == signal.SIGKILL
                and any(s["signal"] == "kill" and s["sent"] for s in last["cleanup"]["signals"]),
                "TERM-ignoring fixture must be killed")
    if item.get("descendant"):
        descendants = list(directory.glob("descendant-*.json"))
        require(descendants and commands[0]["exitCode"] == 0 and commands[0]["termSignal"] is None,
                "leader must really exit before pipe-owning descendant")
    if item.get("signalStep"):
        command = next(c for c in commands if c["step"] == item["signalStep"])
        require(command["exitCode"] is None and command["termSignal"] is not None, "signal exit not preserved")


def execute(args, data):
    report_path = Path(args.report)
    require(report_path.is_absolute(), "--report must be absolute")
    probe = Path(args.probe)
    engine = Path(args.engine)
    require(probe.is_absolute() and probe.is_file() and os.access(probe, os.X_OK), "--probe must be an existing absolute executable")
    require(engine.is_absolute() and engine.is_file() and os.access(engine, os.X_OK), "mandatory --engine is missing/nonexecutable; cannot skip")
    report_path.parent.mkdir(parents=True, exist_ok=True)
    work = Path(tempfile.mkdtemp(prefix="engine-probe-evidence-", dir=str(report_path.parent))).resolve()
    checks = []
    result_report = {
        "schemaVersion": 1, "suite": "engine-probe.v1", "status": "RUNNING", "checks": checks,
        "sourceId": args.source_id, "startedAt": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "environment": {"platform": platform.platform(), "machine": platform.machine(),
                        "python": sys.version, "uid": os.getuid(), "euid": os.geteuid()},
        "probe": {"path": str(probe), "sha256": sha(probe)},
        "realEngine": {"requestedPath": str(engine), "resolvedPath": str(engine.resolve()), "sha256": sha(engine.resolve())},
        "fixtureSha256": {name: sha(FIXTURES / name) for name in ("helper.py", "observations.json")},
        "evidenceDirectory": str(work), "watchdogSeconds": WATCHDOG,
        "coverageLimits": ["NUL cannot be passed in an OS argv; direct API test remains kernel-owned",
                           "No fault injection for fcntl/read/wait/EPERM; kernel internal tests and source review supplement",
                           "No transfer, hostile executable sandbox, escaped process guarantee, FFI or GUI acceptance",
                           "Per-source core119/native27/workspace regression is a separate required root check"],
    }

    def retain(directory, result):
        (directory / "probe.stdout").write_bytes(result["stdout"])
        (directory / "probe.stderr").write_bytes(result["stderr"])
        save(directory / "invocation.json", {k: v for k, v in result.items() if k not in ("stdout", "stderr")})

    def record(name, body):
        started = time.monotonic()
        entry = {"id": name, "status": "RUNNING"}
        checks.append(entry)
        try:
            body(entry)
            entry["status"] = "PASS"
        except Exception as failure:
            entry.update(status="FAIL", detail=type(failure).__name__ + ": " + str(failure))
        entry["elapsedSeconds"] = round(time.monotonic() - started, 3)
        print(entry["status"] + " " + name, flush=True)
        save(report_path, result_report)

    # This exact launch environment comes from QA, without passing through Rust.
    # Distinguish interpreter/runtime additions from a production env_clear bug.
    calibration = work / "fixture-runtime-calibration"
    first = cases(data)[0]
    calibration_executable, _, _ = make_fixture(calibration, first)
    calibration_result = captured([str(calibration_executable), "--version"], Path("/"), ENVIRONMENT)
    retain(calibration, calibration_result)
    require(calibration_result["exitCode"] == 0 and not calibration_result["watchdogTriggered"],
            "fixture interpreter calibration failed")
    calibration_event = load_json((calibration / "version-event.json").read_bytes())
    calibrated_environment = calibration_event["environment"]
    require(all(calibrated_environment.get(k) == v for k, v in ENVIRONMENT.items()), "interpreter changed fixed environment")
    runtime_extra = {k: v for k, v in calibrated_environment.items() if k not in ENVIRONMENT}
    allowed_extra = {"__CF_USER_TEXT_ENCODING"} if sys.platform == "darwin" else set()
    require(set(runtime_extra) <= allowed_extra, "unexpected fixture runtime environment augmentation")
    result_report["fixtureRuntimeAddedEnvironment"] = runtime_extra

    for index, item in enumerate(cases(data)):
        def body(entry, item=item, index=index):
            directory = work / ("%02d-" % index + item["id"])
            entry["evidence"] = str(directory)
            executable, requested, scenario = make_fixture(directory, item)
            before = integrity(directory)
            engine_before = sha(executable)
            inherited = {"PATH": "/must-not-inherit", "LC_ALL": "C", "LANG": "invalid-parent-locale",
                         "TZ": "Pacific/Auckland", "HOME": str(directory), "USER": "qa-synthetic",
                         "RSYNC_RSH": "must-not-be-inherited", "CTSF_QA_SENTINEL": "must-not-be-inherited"}
            result = captured([str(probe), "inspect-engine", requested], directory, inherited)
            retain(directory, result)
            cleanup = cleanup_fixture(directory, result)
            save(directory / "qa-cleanup.json", cleanup)
            entry["qaCleanup"] = cleanup
            require(not cleanup["requested"], "QA fallback cleanup was required; not a production-cleanup pass")
            unverified = [p for p in cleanup["before"] if p["state"] == "unverified"]
            require(not unverified, "fixture process state could not be independently observed")
            require(integrity(directory) == before, "source/destination contents, entries, modes or mtimes changed")
            if not item["scenario"]["version"].get("mutateExecutable"):
                require(sha(executable) == engine_before, "fixture engine was changed")
            require(not (directory / "unexpected-argv.json").exists(), "production requested non-probe arguments")
            value = envelope(result)
            check_fixture(item, value, result, directory, executable, requested, scenario, runtime_extra)
        record(item["id"], body)

    def real_engine(entry):
        directory = work / "real-installed-engine"
        directory.mkdir()
        result = captured([str(probe), "inspect-engine", str(engine)], directory)
        retain(directory, result)
        value = envelope(result)
        require(value["report"] is not None, "real engine lost report")
        commands = validate_report(value["report"], str(engine), str(engine.resolve()))
        require(len(commands) == 2 and commands[0]["exitCode"] == 0, "real version/help must both be observed")
        require(value["report"]["identity"]["kind"] in ("rsync", "openrsync"), "real installed engine must be identified")
        for command in commands:
            require(command["cleanup"]["childReaped"] and command["cleanup"]["group"] == "gone"
                    and all(command[s]["eof"] for s in ("stdout", "stderr")), "real engine cleanup")
        if commands[1]["exitCode"] != 0 or commands[1]["termSignal"] is not None:
            require(value["error"] is not None and value["error"]["code"] == "non_zero_exit"
                    and value["error"]["step"] == "help", "real nonzero help classification")
        else:
            require(value["error"] is None, "real normal observation must succeed")
        entry["evidence"] = str(directory)
        result_report["realEngine"]["observation"] = value
    record("real-installed-engine-mandatory", real_engine)

    def usage(entry):
        directory = work / "inspect-cli-usage"
        directory.mkdir()
        for index, arguments in enumerate([["inspect-engine"], ["inspect-engine", str(engine), "--dry-run"]]):
            result = captured([str(probe)] + arguments, directory)
            sample = directory / str(index)
            sample.mkdir()
            retain(sample, result)
            value = envelope(result)
            require(value["error"] is not None and value["error"]["code"] == "invalid_engine_path"
                    and value["error"]["step"] is None and value["report"] is None,
                    "invalid inspect CLI must fail before spawn")
        entry["evidence"] = str(directory)
    record("inspect-cli-rejects-missing-and-extra-arguments", usage)

    def compatibility(entry):
        directory = work / "legacy-health-and-planner"
        directory.mkdir()
        health = captured([str(probe)], directory)
        require(health["exitCode"] == 0 and not health["watchdogTriggered"] and health["stderr"] == b""
                and health["stdout"] == ("ctsf-kernel version=" + args.expected_version + " contract=setup.v1 health=ready\n").encode(),
                "legacy setup health changed")
        (directory / "health.stdout").write_bytes(health["stdout"])
        (directory / "src").mkdir()
        (directory / "dst").mkdir()
        marker = directory / "never-executed"
        marker.write_text("#!/bin/sh\nprintf executed > \"$0.executed\"\n", encoding="utf-8")
        marker.chmod(0o755)
        task = {"schemaVersion": 1, "kind": "rsync.task.pot", "taskId": "engine-probe-regression",
                "revision": 1, "name": "Legacy planner remains pure", "engineExecutable": str(marker),
                "source": {"raw": str(directory / "src") + "/"}, "destination": {"raw": str(directory / "dst")},
                "options": [{"kind": "recursive"}]}
        save(directory / "task.json", task)
        plan = captured([str(probe), "plan", str(directory / "task.json")], directory)
        retain(directory, plan)
        require(plan["exitCode"] == 0 and plan["stderr"] == b"" and not plan["watchdogTriggered"], "legacy plan failed")
        value = load_json(plan["stdout"])
        require(value["contractVersion"] == "planner.v1" and value["engineCheck"] == "not_performed", "legacy planner contract")
        require(value["runArgv"] == ["--recursive", "--", task["source"]["raw"], task["destination"]["raw"]]
                and value["dryRunArgv"] == ["--recursive", "--dry-run", "--", task["source"]["raw"], task["destination"]["raw"]],
                "legacy planner ordered argv changed")
        require(not Path(str(marker) + ".executed").exists(), "pure planning executed marker engine")
        entry["evidence"] = str(directory)
    record("legacy-health-planner-and-no-transfer", compatibility)
    record("probe-binary-unchanged", lambda _: require(sha(probe) == result_report["probe"]["sha256"], "probe changed during run"))
    counts = {status: sum(c["status"] == status for c in checks) for status in ("PASS", "FAIL", "UNVERIFIED")}
    result_report.update(status="FAIL" if counts["FAIL"] else "PASS", counts=counts,
                         finishedAt=datetime.datetime.now(datetime.timezone.utc).isoformat())
    save(report_path, result_report)
    print(json.dumps({"status": result_report["status"], "counts": counts, "report": str(report_path)}))
    return 1 if counts["FAIL"] else 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--probe")
    parser.add_argument("--engine")
    parser.add_argument("--report")
    parser.add_argument("--expected-version", default="0.1.0")
    parser.add_argument("--source-id")
    parser.add_argument("--check-fixtures", action="store_true")
    args = parser.parse_args()
    try:
        data = prepared()
        if args.check_fixtures:
            print(json.dumps({"status": "PREPARED_ONLY", "probeExecuted": False,
                              "fixtureCases": len(cases(data)), "watchdogSeconds": WATCHDOG}))
            return 0
        if not all((args.probe, args.engine, args.report)):
            parser.error("--probe, --engine and --report are required for actual acceptance")
        return execute(args, data)
    except Exception as failure:
        error = {"status": "FAIL", "checks": [], "counts": {"PASS": 0, "FAIL": 1, "UNVERIFIED": 0},
                 "fatalError": type(failure).__name__ + ": " + str(failure)}
        if args.report and Path(args.report).is_absolute():
            Path(args.report).parent.mkdir(parents=True, exist_ok=True)
            save(Path(args.report), error)
        print(json.dumps(error), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
