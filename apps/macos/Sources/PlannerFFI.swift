import Dispatch
import Foundation

enum PlannerFFI {
    static func plan(_ input: PlannerTaskValue) -> PlannerOutcome {
        dispatchPrecondition(condition: .notOnQueue(.main))
        precondition(!Thread.isMainThread, "Directory planning must run outside the main thread")
        do {
            return .success(value(try buildLocalPlan(task: task(input))))
        } catch let error as PlannerBridgeError {
            switch error {
            case .Planning(let code, let field, let message, let osCode):
                return .failure(
                    .planning(
                        code: failureCode(code), field: field, message: message, osCode: osCode))
            }
        } catch {
            return .failure(.bridge(message: String(reflecting: error)))
        }
    }

    private static func task(_ input: PlannerTaskValue) -> PlannerTask {
        PlannerTask(
            schemaVersion: input.schemaVersion, kind: input.kind, taskId: input.taskID,
            revision: input.revision, name: input.name, engineExecutable: input.engineExecutable,
            source: PlannerLocalPath(raw: input.sourceRaw),
            destination: PlannerLocalPath(raw: input.destinationRaw),
            options: input.options.map {
                switch $0 {
                case .recursive: .recursive
                case .archive: .archive
                }
            })
    }

    private static func value(_ task: PlannerTask) -> PlannerTaskValue {
        PlannerTaskValue(
            schemaVersion: task.schemaVersion, kind: task.kind, taskID: task.taskId,
            revision: task.revision, name: task.name, engineExecutable: task.engineExecutable,
            sourceRaw: task.source.raw, destinationRaw: task.destination.raw,
            options: task.options.map {
                switch $0 {
                case .recursive: .recursive
                case .archive: .archive
                }
            })
    }

    private static func value(_ plan: PlannerInvocationPlan) -> PlannerPlanValue {
        let pathCheck: PlannerPathCheckValue =
            switch plan.pathCheck {
            case .checkedAtPlanning: .checkedAtPlanning
            }
        let engineCheck: PlannerEngineCheckValue =
            switch plan.engineCheck {
            case .notPerformed: .notPerformed
            }
        return PlannerPlanValue(
            contractVersion: plan.contractVersion, taskSnapshot: value(plan.taskSnapshot),
            executable: plan.executable, runArguments: plan.runArgv,
            dryRunArguments: plan.dryRunArgv, pathCheck: pathCheck, engineCheck: engineCheck)
    }

    private static func failureCode(_ code: PlannerErrorCode) -> PlannerFailureCode {
        switch code {
        case .invalidJson: .invalidJson
        case .unsupportedSchema: .unsupportedSchema
        case .invalidTask: .invalidTask
        case .invalidPath: .invalidPath
        case .unsafePathRelation: .unsafePathRelation
        case .pathAccessDenied: .pathAccessDenied
        case .pathIo: .pathIo
        }
    }
}
