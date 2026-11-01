import Dispatch

protocol PlannerProviding: Sendable {
    func plan(_ input: PlannerTaskValue) async -> PlannerOutcome
}

struct PlannerWorker: PlannerProviding {
    typealias Operation = @Sendable (PlannerTaskValue) -> PlannerOutcome

    private let queue: DispatchQueue
    private let operation: Operation

    init(operation: @escaping Operation) {
        queue = DispatchQueue(label: "ctsf.planner", qos: .userInitiated)
        self.operation = operation
    }

    func plan(_ input: PlannerTaskValue) async -> PlannerOutcome {
        await withCheckedContinuation { continuation in
            queue.async { [operation] in
                continuation.resume(returning: operation(input))
            }
        }
    }
}
