import Dispatch
import Foundation

private func nextQAEvent<Value: Sendable>(_ events: AsyncStream<Value>) async throws -> Value {
    try await withThrowingTaskGroup(of: Value.self) { group in
        group.addTask {
            var iterator = events.makeAsyncIterator()
            guard let value = await iterator.next() else { throw QAProblem("event stream ended") }
            return value
        }
        group.addTask {
            try await Task.sleep(for: .seconds(5))
            throw QAProblem("gate/event handshake timed out")
        }
        defer { group.cancelAll() }
        guard let result = try await group.next() else { throw QAProblem("missing gate result") }
        return result
    }
}

private struct WorkerEvent: Sendable {
    let stage: String
    let onMainThread: Bool
    let input: PlannerTaskValue
    let outcome: PlannerOutcome?
}

private final class RealWorkerGate: Sendable {
    let events: AsyncStream<WorkerEvent>
    private let eventSink: AsyncStream<WorkerEvent>.Continuation
    private let release = DispatchSemaphore(value: 0)

    init() {
        let pair = AsyncStream<WorkerEvent>.makeStream()
        events = pair.stream
        eventSink = pair.continuation
    }

    func open() { release.signal() }

    func operation(_ input: PlannerTaskValue) -> PlannerOutcome {
        eventSink.yield(
            WorkerEvent(
                stage: "entered", onMainThread: Thread.isMainThread, input: input, outcome: nil))
        release.wait()  // Only the production worker's private queue waits here.
        eventSink.yield(
            WorkerEvent(
                stage: "real-ffi-call", onMainThread: Thread.isMainThread, input: input,
                outcome: nil))
        let outcome = PlannerFFI.plan(input)
        eventSink.yield(
            WorkerEvent(
                stage: "returned", onMainThread: Thread.isMainThread, input: input, outcome: outcome
            ))
        return outcome
    }
}

private struct ControlledCall: Sendable {
    let index: Int
    let input: PlannerTaskValue
}

// This controls transport ordering, not the production acceptance state machine.
private actor ControlledProvider: PlannerProviding {
    nonisolated let events: AsyncStream<ControlledCall>
    private let eventSink: AsyncStream<ControlledCall>.Continuation
    private var waiting: [Int: CheckedContinuation<PlannerOutcome, Never>] = [:]
    private var count = 0

    init() {
        let pair = AsyncStream<ControlledCall>.makeStream()
        events = pair.stream
        eventSink = pair.continuation
    }

    func plan(_ input: PlannerTaskValue) async -> PlannerOutcome {
        let index = count
        count += 1
        return await withCheckedContinuation { continuation in
            waiting[index] = continuation
            eventSink.yield(ControlledCall(index: index, input: input))
        }
    }

    func finish(_ call: ControlledCall, with outcome: PlannerOutcome) throws {
        guard let continuation = waiting.removeValue(forKey: call.index) else {
            throw QAProblem("test provider must resume each continuation exactly once")
        }
        continuation.resume(returning: outcome)
    }

    func callCount() -> Int { count }
}

@MainActor
private final class QAAsyncChecks {
    private(set) var values: [QACheck] = []

    func run(_ id: String, _ operation: @MainActor () async throws -> Void) async {
        do {
            try await operation()
            values.append(QACheck(id: id, status: "PASS", detail: ""))
        } catch {
            values.append(QACheck(id: id, status: "FAIL", detail: String(describing: error)))
        }
    }
}

@MainActor
private func watch(_ coordinator: PlannerCoordinator) -> AsyncStream<PlannerViewState> {
    let pair = AsyncStream<PlannerViewState>.makeStream()
    coordinator.onStateChange = { state in pair.continuation.yield(state) }
    return pair.stream
}

@MainActor
private func waitUntilIdle(
    _ coordinator: PlannerCoordinator, _ events: AsyncStream<PlannerViewState>
) async throws {
    while coordinator.state.inFlight != nil { _ = try await nextQAEvent(events) }
}

private func formFor(_ fixture: NativePlannerFixture) -> PlannerFormValues {
    PlannerFormValues(
        engineExecutable: fixture.missingEngine, sourceRaw: fixture.source,
        destinationRaw: fixture.destination, option: .recursive)
}

private func requireSuccess(_ outcome: PlannerOutcome) throws -> PlannerPlanValue {
    guard case .success(let plan) = outcome else {
        throw QAProblem("real operation did not produce a plan: \(outcome)")
    }
    return plan
}

private func changedPlan(
    _ plan: PlannerPlanValue, contract: String? = nil, snapshot: PlannerTaskValue? = nil
) -> PlannerPlanValue {
    PlannerPlanValue(
        contractVersion: contract ?? plan.contractVersion,
        taskSnapshot: snapshot ?? plan.taskSnapshot,
        executable: plan.executable, runArguments: plan.runArguments,
        dryRunArguments: plan.dryRunArguments,
        pathCheck: plan.pathCheck, engineCheck: plan.engineCheck)
}

@MainActor
func runProductionGateChecks(assets: URL) async throws -> QAReport {
    let fixture = try NativePlannerFixture(assets: assets)
    defer { fixture.remove() }
    let initialManifest = try fixture.manifest()
    let checks = QAAsyncChecks()
    let realWorker = PlannerWorker(operation: { input in PlannerFFI.plan(input) })

    await checks.run("production-worker-real-ffi-off-main-and-aba-single-flight") {
        let gate = RealWorkerGate()
        defer { gate.open() }
        let worker = PlannerWorker(operation: { input in gate.operation(input) })
        let coordinator = PlannerCoordinator(provider: worker)
        let states = watch(coordinator)
        let formA = formFor(fixture)
        coordinator.updateForm(formA)
        guard let first = coordinator.generate() else { throw QAProblem("first Generate rejected") }
        try qaRequire(coordinator.generate() == nil, "double Generate accepted another request")
        let entered = try await nextQAEvent(gate.events)
        try qaRequire(
            entered.stage == "entered" && !entered.onMainThread, "worker entered on main thread")
        try qaRequire(
            entered.input.revision == first.formRevision,
            "worker did not capture the input revision")
        var formB = formA
        formB.sourceRaw += "/"
        coordinator.updateForm(formB)
        coordinator.updateForm(formA)
        try qaRequire(
            coordinator.state.formRevision > first.formRevision, "A→B→A did not advance revision")
        try qaRequire(coordinator.state.inFlight == first, "editing released the in-flight slot")
        try qaRequire(
            coordinator.generate() == nil && !coordinator.state.canCopy,
            "busy request/copy gate failed")
        // An actual main-queue event must run while the worker semaphore is closed.
        await withCheckedContinuation { continuation in
            DispatchQueue.main.async { continuation.resume() }
        }
        gate.open()
        let calling = try await nextQAEvent(gate.events)
        try qaRequire(
            calling.stage == "real-ffi-call" && !calling.onMainThread,
            "actual FFI call point was on main")
        let returned = try await nextQAEvent(gate.events)
        _ = try requireSuccess(try requireOutcome(returned.outcome))
        try await waitUntilIdle(coordinator, states)
        try qaRequire(coordinator.state.phase == .outOfDate, "stale success became current")
        try qaRequire(
            coordinator.state.preview == nil && !coordinator.state.canCopy
                && coordinator.state.failure == nil, "stale success was published")
        try qaRequire(
            coordinator.state.canGenerate, "stale completion did not clear its own busy slot")
        guard let second = coordinator.generate() else {
            throw QAProblem("explicit subsequent Generate rejected")
        }
        try qaRequire(second.requestSequence > first.requestSequence, "request sequence was reused")
        let secondEntry = try await nextQAEvent(gate.events)
        try qaRequire(
            secondEntry.input.revision == coordinator.state.formRevision,
            "new request captured old revision")
        gate.open()
        _ = try await nextQAEvent(gate.events)
        _ = try await nextQAEvent(gate.events)
        try await waitUntilIdle(coordinator, states)
        try qaRequire(
            coordinator.state.canCopy && coordinator.state.previewIsCurrent,
            "current success not accepted")
        coordinator.close()
    }

    await checks.run("stale-real-error-is-discarded-without-automatic-retry") {
        let provider = ControlledProvider()
        let coordinator = PlannerCoordinator(provider: provider)
        let states = watch(coordinator)
        var badForm = formFor(fixture)
        badForm.sourceRaw = fixture.root.appendingPathComponent("missing").path
        coordinator.updateForm(badForm)
        try qaRequire(coordinator.generate() != nil, "complete invalid form should reach Rust")
        let call = try await nextQAEvent(provider.events)
        coordinator.updateForm(formFor(fixture))
        let outcome = await realWorker.plan(call.input)
        guard case .failure(.planning(let code, let field, _, let osCode)) = outcome else {
            throw QAProblem("expected real typed filesystem failure")
        }
        try qaRequire(
            code == .invalidPath && field == "source.raw" && osCode != nil,
            "production error mapping lost fields")
        try await provider.finish(call, with: outcome)
        try await waitUntilIdle(coordinator, states)
        try qaRequire(
            coordinator.state.failure == nil && coordinator.state.preview == nil,
            "stale error was published")
        try qaRequire(
            coordinator.state.phase == .outOfDate && coordinator.state.canGenerate,
            "stale error left busy stuck")
        try qaRequire(await provider.callCount() == 1, "coordinator automatically retried/queued")
        coordinator.close()
    }

    await checks.run("duplicate-old-success-and-error-do-not-clear-new-busy") {
        let provider = ControlledProvider()
        let coordinator = PlannerCoordinator(provider: provider)
        let states = watch(coordinator)
        coordinator.updateForm(formFor(fixture))
        guard let firstToken = coordinator.generate() else {
            throw QAProblem("first Generate rejected")
        }
        let firstCall = try await nextQAEvent(provider.events)
        let firstOutcome = await realWorker.plan(firstCall.input)
        _ = try requireSuccess(firstOutcome)
        try await provider.finish(firstCall, with: firstOutcome)
        try await waitUntilIdle(coordinator, states)
        guard let nextToken = coordinator.generate() else {
            throw QAProblem("same-revision Generate rejected")
        }
        let nextCall = try await nextQAEvent(provider.events)
        try qaRequire(
            nextToken.formRevision == firstToken.formRevision
                && nextToken.requestSequence > firstToken.requestSequence,
            "same revision request token is not distinct")
        coordinator.receive(PlannerCompletion(token: firstToken, outcome: firstOutcome))
        coordinator.receive(
            PlannerCompletion(
                token: firstToken, outcome: .failure(.bridge(message: "duplicate old error"))))
        try qaRequire(
            coordinator.state.inFlight == nextToken && coordinator.state.phase == .generating,
            "old completion cleared new busy")
        try qaRequire(
            !coordinator.state.canCopy && coordinator.state.failure == nil,
            "duplicate old completion changed current feedback")
        let current = await realWorker.plan(nextCall.input)
        try await provider.finish(nextCall, with: current)
        try await waitUntilIdle(coordinator, states)
        try qaRequire(coordinator.state.canCopy, "current response was not accepted")
        coordinator.close()
    }

    await checks.run("current-failure-clears-previous-preview-after-directory-removal") {
        let transient = fixture.root.appendingPathComponent("transient")
        try FileManager.default.createDirectory(at: transient, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: transient) }
        var form = formFor(fixture)
        form.sourceRaw = transient.path
        let provider = ControlledProvider()
        let coordinator = PlannerCoordinator(provider: provider)
        let states = watch(coordinator)
        coordinator.updateForm(form)
        try qaRequire(coordinator.generate() != nil, "first Generate rejected")
        let first = try await nextQAEvent(provider.events)
        let success = await realWorker.plan(first.input)
        _ = try requireSuccess(success)
        try await provider.finish(first, with: success)
        try await waitUntilIdle(coordinator, states)
        try qaRequire(coordinator.state.canCopy, "first real plan missing")
        try FileManager.default.removeItem(at: transient)
        try qaRequire(coordinator.generate() != nil, "regenerate rejected")
        let second = try await nextQAEvent(provider.events)
        let failure = await realWorker.plan(second.input)
        guard case .failure(.planning(let code, _, _, _)) = failure, code == .invalidPath else {
            throw QAProblem("directory deletion did not produce a real InvalidPath")
        }
        try await provider.finish(second, with: failure)
        try await waitUntilIdle(coordinator, states)
        try qaRequire(
            coordinator.state.phase == .failed && coordinator.state.preview == nil
                && !coordinator.state.canCopy, "current failure resurrected previous preview")
        coordinator.close()
    }

    for invalid in ["contract", "snapshot"] {
        await checks.run("reject-current-incorrect-\(invalid)") {
            let provider = ControlledProvider()
            let coordinator = PlannerCoordinator(provider: provider)
            let states = watch(coordinator)
            coordinator.updateForm(formFor(fixture))
            try qaRequire(coordinator.generate() != nil, "Generate rejected")
            let call = try await nextQAEvent(provider.events)
            let real = try requireSuccess(await realWorker.plan(call.input))
            let wrongSnapshot = PlannerTaskValue(
                schemaVersion: call.input.schemaVersion, kind: call.input.kind,
                taskID: call.input.taskID,
                revision: call.input.revision + 1, name: call.input.name,
                engineExecutable: call.input.engineExecutable, sourceRaw: call.input.sourceRaw,
                destinationRaw: call.input.destinationRaw, options: call.input.options)
            let changed =
                invalid == "contract"
                ? changedPlan(real, contract: "wrong-contract")
                : changedPlan(real, snapshot: wrongSnapshot)
            try await provider.finish(call, with: .success(changed))
            try await waitUntilIdle(coordinator, states)
            guard case .bridge = coordinator.state.failure else {
                throw QAProblem("incorrect response was not a bridge failure")
            }
            try qaRequire(
                coordinator.state.preview == nil && !coordinator.state.canCopy,
                "incorrect response became a valid preview")
            coordinator.close()
        }
    }

    await checks.run("byte-distinct-edit-and-copy-json-use-real-returned-values") {
        let coordinator = PlannerCoordinator(provider: realWorker)
        let states = watch(coordinator)
        let form = formFor(fixture)
        coordinator.updateForm(form)
        let revision = coordinator.state.formRevision
        var composed = form
        composed.sourceRaw = form.sourceRaw.precomposedStringWithCanonicalMapping
        try qaRequire(
            Array(composed.sourceRaw.utf8) != Array(form.sourceRaw.utf8),
            "normalization fixture must have different bytes")
        coordinator.updateForm(composed)
        try qaRequire(
            coordinator.state.formRevision > revision, "Unicode-equivalent byte edit was ignored")
        coordinator.updateForm(form)
        try qaRequire(coordinator.generate() != nil, "Generate rejected")
        try await waitUntilIdle(coordinator, states)
        guard let plan = coordinator.state.preview, coordinator.state.canCopy else {
            throw QAProblem("real current preview missing")
        }
        for args in [plan.runArguments, plan.dryRunArguments] {
            let text = try PlannerText.copyJSON(executable: plan.executable, arguments: args)
            let data = try JSONSerialization.jsonObject(with: Data(text.utf8))
            guard let object = data as? [String: Any], Set(object.keys) == ["executable", "args"],
                let executable = object["executable"] as? String,
                let copied = object["args"] as? [String]
            else { throw QAProblem("copy helper did not produce the specified JSON object") }
            try qaBytesEqual(executable, plan.executable, "copied executable")
            try qaTokensEqual(copied, args, "copied args")
        }
        let currentRevision = coordinator.state.formRevision
        coordinator.updateForm(form)  // Same value models a cancelled/no-change selection.
        try qaRequire(
            coordinator.state.formRevision == currentRevision && coordinator.state.canCopy,
            "unchanged input invalidated a current plan")
        coordinator.close()
    }

    await checks.run("close-rejects-late-completion-and-releases-ui-owner") {
        let provider = ControlledProvider()
        var coordinator: PlannerCoordinator? = PlannerCoordinator(provider: provider)
        var publications = 0
        coordinator?.onStateChange = { _ in publications += 1 }
        let ownerIsReleased: @MainActor () -> Bool = { [weak coordinator] in coordinator == nil }
        coordinator?.updateForm(formFor(fixture))
        guard let token = coordinator?.generate() else { throw QAProblem("Generate rejected") }
        let call = try await nextQAEvent(provider.events)
        let outcome = await realWorker.plan(call.input)
        coordinator?.close()
        let closedPublications = publications
        coordinator?.receive(PlannerCompletion(token: token, outcome: outcome))
        try qaRequire(
            coordinator?.state.phase == .closed && coordinator?.state.preview == nil,
            "closed owner published a late preview")
        try qaRequire(
            publications == closedPublications, "closed owner published a late state notification")
        try qaRequire(
            coordinator?.generate() == nil && coordinator?.state.canCopy == false,
            "closed owner accepted work/copy")
        coordinator = nil
        try qaRequire(ownerIsReleased(), "pending request retained the UI owner")
        try await provider.finish(call, with: outcome)
        try qaRequire(await provider.callCount() == 1, "close caused another request")
    }

    await checks.run("gate-fixture-and-engine-remain-unchanged") {
        var before = initialManifest
        var after = try fixture.manifest()
        before.removeValue(forKey: "")
        after.removeValue(forKey: "")
        try qaRequire(before == after, "production worker/gate flow changed fixture data")
        try qaRequire(
            !FileManager.default.fileExists(atPath: fixture.markerEngine.path + ".executed"),
            "engine ran")
    }
    return QAReport(
        scope: "production-worker-coordinator-controlled-transport", checks: checks.values)
}

private func requireOutcome(_ outcome: PlannerOutcome?) throws -> PlannerOutcome {
    guard let outcome else { throw QAProblem("worker returned event without an outcome") }
    return outcome
}
