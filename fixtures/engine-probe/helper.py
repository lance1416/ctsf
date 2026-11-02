"""Executable fixture body; the harness supplies an absolute Python shebang.

This is a chosen test executable, not a production hook. All private control and
evidence files are siblings of this file in a fresh disposable case directory.
"""

import atexit
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import threading
import time


HERE = Path(__file__).resolve().parent
CONFIG = json.loads((HERE / "scenario.json").read_text(encoding="utf-8"))
ROLE = "descendant" if sys.argv[1:] == ["--fixture-descendant"] else "leader"
PID = os.getpid()


def record(name, value):
    target = HERE / name
    temporary = target.with_suffix(".tmp-" + str(PID))
    temporary.write_text(json.dumps(value, ensure_ascii=True), encoding="utf-8")
    temporary.replace(target)


def guard():
    # Allows QA to stop its fixtures without signalling a stale numeric PGID.
    while True:
        if (HERE / "qa-stop").exists():
            record("qa-stopped-" + str(PID) + ".json", {"pid": PID})
            os._exit(91)
        time.sleep(0.02)


threading.Thread(target=guard, daemon=True).start()
atexit.register(lambda: record("exited-" + str(PID) + ".json", {"pid": PID}))

if ROLE == "descendant":
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
    record("descendant-" + str(PID) + ".json", {
        "pid": PID, "pgid": os.getpgrp(), "role": ROLE, "nonce": CONFIG["nonce"],
    })
    while True:
        time.sleep(0.02)

if sys.argv[1:] not in (["--version"], ["--help"]):
    record("unexpected-argv.json", {"argv": sys.argv})
    raise SystemExit(93)

STEP = "version" if sys.argv[1] == "--version" else "help"
SPEC = CONFIG.get(STEP, {})
record(STEP + "-event.json", {
    "pid": PID, "pgid": os.getpgrp(), "role": ROLE, "nonce": CONFIG["nonce"],
    "argv": sys.argv, "cwd": os.getcwd(), "environment": dict(os.environ),
    "stdin": list(os.read(0, 1)), "monotonic": time.monotonic(),
})

if SPEC.get("ignoreTerm"):
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
if SPEC.get("descendant"):
    subprocess.Popen([sys.executable, str(Path(__file__).resolve()), "--fixture-descendant"])
    until = time.monotonic() + 2
    while not list(HERE.glob("descendant-*.json")):
        if time.monotonic() >= until:
            raise SystemExit(94)
        time.sleep(0.005)
if SPEC.get("closePipes"):
    os.close(1)
    os.close(2)
if SPEC.get("mutateExecutable"):
    with Path(__file__).open("ab") as handle:
        handle.write(b"\n# fixture deliberately changed between commands\n")
if SPEC.get("delay"):
    time.sleep(SPEC["delay"])
if SPEC.get("signal"):
    os.kill(PID, getattr(signal, SPEC["signal"]))


def write(fd, key):
    data = bytes(SPEC[key + "Bytes"]) if key + "Bytes" in SPEC else SPEC.get(key, "").encode("utf-8")
    offset = 0
    while offset < len(data):
        offset += os.write(fd, data[offset:offset + 4096])


if SPEC.get("dualFlood"):
    os.write(1, b"stdout-ready\n")
    os.write(2, b"stderr-ready\n")
    threads = [threading.Thread(target=write, args=(fd, key)) for fd, key in [(1, "stdout"), (2, "stderr")]]
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join()
elif not SPEC.get("closePipes"):
    write(1, "stdout")
    write(2, "stderr")
raise SystemExit(SPEC.get("exit", 0))
