# Native planner acceptance

The default harness executes real typed UniFFI checks and the actual production
worker/coordinator through narrow test gates. It does not import SwiftUI or copy
the production state machine. No Rust test/debug API is added.

Compile these exact QA files, plus the version-matched generated Swift and static
Rust archive:

- `tests/native-planner/TypedBridgeChecks.swift`
- `tests/native-planner/GateChecks.swift`
- `tests/native-planner/NativePlannerAcceptance.swift` — the only test `@main`

The production shared inputs are `PlannerValues.swift`, `PlannerWorker.swift`,
`PlannerCoordinator.swift` and `PlannerFFI.swift` under `apps/macos/Sources/`.
Do not glob the app entry point, AppUI, other smoke mains or all Tests sources.
Missing inputs must fail compilation; the root build command is owned by the
build maintainer. Use Swift 6, complete concurrency checking and warnings as
errors, and the same target/deployment settings as the app.

Runtime CLI:

```text
NativePlannerAcceptance /absolute/path/to/fixtures/native-planner expected-kernel-version
```

The optional leading `--typed-only` runs only the typed component and identifies
that narrower scope in its JSON. It is not full worker/coordinator or UI acceptance.
The normal JSON scope is `real-typed-bridge-and-production-worker-coordinator`.
FAIL returns nonzero. Permission preconditions that cannot be established are
UNVERIFIED and yield PASS_WITH_LIMITATIONS rather than claiming permission passed.

The task template tests UInt64.max and decomposed Unicode; expected argv tokens
are handwritten in `fixtures/native-planner/argv-oracle.json`. UTF-8 bytes are
compared explicitly. A marker engine is copied into disposable data and never
launched by the harness. Permission errno comes from a Darwin SDK metadata call
against the harness's own blocked directory; permissions are restored afterwards.
The harness does not alter TCC or any system settings.

Worker gates wait only on the production private queue, then invoke the real
`PlannerFFI.plan` at the measured call point. MainActor event progress is tested
while the gate is closed. Controlled provider continuations only control reply
order; all acceptance rules belong to `PlannerCoordinator`. Each continuation
resumes once; replayed old completions go through its production receiver.
Timeouts guard missing handshakes, not simulate scheduling with fixed sleeps.

Real-window/AX, native directory panels, keyboard/VoiceOver, pasteboard interaction
and per-commit dual-platform regression are separate required evidence. A passing
CLI or screenshot alone does not mark the complete native feature accepted.
