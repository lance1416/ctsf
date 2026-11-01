@MainActor
final class PlannerCoordinator {
    private let provider: any PlannerProviding
    private var activeInput: PlannerTaskValue?

    private(set) var state = PlannerViewState()
    var onStateChange: (@MainActor (PlannerViewState) -> Void)?

    init(provider: any PlannerProviding) {
        self.provider = provider
    }

    func updateForm(_ form: PlannerFormValues) {
        guard state.windowIsOpen, form != state.form else { return }
        guard state.formRevision < UInt64.max else {
            state.failure = .bridge(message: "The form revision counter has been exhausted.")
            publish()
            return
        }
        state.form = form
        state.formRevision += 1
        state.previewIsCurrent = false
        state.failure = nil
        state.phase = .outOfDate
        publish()
    }

    @discardableResult
    func generate() -> PlannerRequestToken? {
        guard state.canGenerate else { return nil }
        state.requestSequence += 1
        let token = PlannerRequestToken(
            requestSequence: state.requestSequence, formRevision: state.formRevision)
        let input = PlannerTaskValue(form: state.form, revision: state.formRevision)
        state.inFlight = token
        activeInput = input
        state.previewIsCurrent = false
        state.failure = nil
        state.phase = .generating
        publish()

        Task { @MainActor [weak self, provider] in
            let outcome = await provider.plan(input)
            self?.receive(PlannerCompletion(token: token, outcome: outcome))
        }
        return token
    }

    // The production completion receiver also lets an independent harness replay events.
    func receive(_ completion: PlannerCompletion) {
        guard state.windowIsOpen, state.inFlight == completion.token,
            let expected = activeInput
        else { return }

        state.inFlight = nil
        activeInput = nil
        guard completion.token.formRevision == state.formRevision else {
            state.phase = .outOfDate
            publish()
            return
        }

        state.acceptedResponseSequence = completion.token.requestSequence
        switch completion.outcome {
        case .success(let plan):
            guard plannerSameText(plan.contractVersion, "planner.v1"),
                plan.taskSnapshot == expected
            else {
                fail(.bridge(message: "The returned plan contract or task snapshot did not match."))
                return
            }
            state.preview = plan
            state.previewIsCurrent = true
            state.failure = nil
            state.phase = .generated
        case .failure(let failure):
            fail(failure)
            return
        }
        publish()
    }

    func close() {
        guard state.windowIsOpen else { return }
        state.windowIsOpen = false
        state.inFlight = nil
        activeInput = nil
        state.previewIsCurrent = false
        state.phase = .closed
        publish()
    }

    private func fail(_ failure: PlannerFailureValue) {
        state.preview = nil
        state.previewIsCurrent = false
        state.failure = failure
        state.phase = .failed
        publish()
    }

    private func publish() {
        onStateChange?(state)
    }
}
