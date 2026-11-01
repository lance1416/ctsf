# Independent planner acceptance

Build the real probe first, then run the Python standard-library harness:

```sh
cargo build --locked -p kernel-probe
python3 tests/planner/acceptance.py \
  --probe "$PWD/target/debug/kernel-probe" \
  --report "$PWD/build/planner/qa-report.json"
```

`--probe` is an absolute path to an already-built executable, including on Linux
or hosted CI. The harness does not invoke Cargo, rsync, a shell, or a mock probe.
`--expected-version` defaults to `0.1.0` for the legacy setup health line;
`--source-id` can record a caller-provided commit or source-snapshot identifier.
The report path must be absolute. Without `--report`, a unique temporary evidence
directory is created and printed. Raw JSON inputs are retained beside the report.

`python3 tests/planner/acceptance.py --check-fixtures` checks preparation without
calling a probe. Its `PREPARED_ONLY` result is not product acceptance.

The handwritten fixture oracle checks complete plan records and ordered argv.
Coverage includes strict JSON/duplicate keys/schema/option tags, UTF-8 byte
limits, raw spaces/shell literals/trailing slashes, aliases and directory
relationships, unchanged data, missing/uninspected engines, a never-executed
marker script, stable repeated plans and the real stdout/stderr/exit protocol.
The fixture tree includes inner symlinks that a top-level planner must not scan.
The runner never imports or calls the planner to compute an expected result.

Every invocation runs in fresh disposable test data, with integrity checks before
and after. Permission cases first establish that directory traversal actually
raises PermissionError. If the identity bypasses permissions, those cases are
`UNVERIFIED`; the aggregate is `PASS_WITH_LIMITATIONS`, with explicit counts.
This is not a claim that permission denial passed. Other assertion failures,
crashes, unexpected output, timeouts, and a changed probe binary produce exit 1.
Exit 0 means all observable assertions passed, possibly with the explicitly
reported permission limitation. Missing/nonexecutable probes fail before testing.

PLAN-01 typed-DTO bypass/ownership and direct serde API behavior need developer
tests; a short-lived CLI cannot independently prove them. PLAN-07 needs real
platform/build evidence, and PLAN-08 needs the workspace and applicable old FFI
checks. The JSON report records these limits rather than marking all eight PLAN
requirements passed. No actual transfer/Dry Run/engine capability, cancellation,
runtime or native planner UI is exercised. The accepted native feature is not
copied or modified by this harness.
