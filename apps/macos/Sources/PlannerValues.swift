import Foundation

func plannerSameText(_ lhs: String, _ rhs: String) -> Bool {
    lhs.utf8.elementsEqual(rhs.utf8)
}

private func plannerSameTexts(_ lhs: [String], _ rhs: [String]) -> Bool {
    lhs.count == rhs.count && zip(lhs, rhs).allSatisfy { plannerSameText($0, $1) }
}

enum PlannerOptionValue: String, CaseIterable, Sendable {
    case recursive
    case archive

    var label: String {
        switch self {
        case .recursive: "Recursive (--recursive)"
        case .archive: "Archive (--archive)"
        }
    }
}

struct PlannerFormValues: Sendable, Equatable {
    var engineExecutable = ""
    var sourceRaw = ""
    var destinationRaw = ""
    var option: PlannerOptionValue?

    var isComplete: Bool {
        !engineExecutable.isEmpty && !sourceRaw.isEmpty && !destinationRaw.isEmpty && option != nil
    }

    static func == (lhs: Self, rhs: Self) -> Bool {
        plannerSameText(lhs.engineExecutable, rhs.engineExecutable)
            && plannerSameText(lhs.sourceRaw, rhs.sourceRaw)
            && plannerSameText(lhs.destinationRaw, rhs.destinationRaw)
            && lhs.option == rhs.option
    }
}

struct PlannerTaskValue: Sendable, Equatable {
    let schemaVersion: UInt32
    let kind: String
    let taskID: String
    let revision: UInt64
    let name: String
    let engineExecutable: String
    let sourceRaw: String
    let destinationRaw: String
    let options: [PlannerOptionValue]

    init(form: PlannerFormValues, revision: UInt64) {
        schemaVersion = 1
        kind = "rsync.task.pot"
        taskID = "macos-local-plan"
        self.revision = revision
        name = "Local plan"
        engineExecutable = form.engineExecutable
        sourceRaw = form.sourceRaw
        destinationRaw = form.destinationRaw
        options = form.option.map { [$0] } ?? []
    }

    init(
        schemaVersion: UInt32, kind: String, taskID: String, revision: UInt64, name: String,
        engineExecutable: String, sourceRaw: String, destinationRaw: String,
        options: [PlannerOptionValue]
    ) {
        self.schemaVersion = schemaVersion
        self.kind = kind
        self.taskID = taskID
        self.revision = revision
        self.name = name
        self.engineExecutable = engineExecutable
        self.sourceRaw = sourceRaw
        self.destinationRaw = destinationRaw
        self.options = options
    }

    static func == (lhs: Self, rhs: Self) -> Bool {
        lhs.schemaVersion == rhs.schemaVersion && lhs.revision == rhs.revision
            && plannerSameText(lhs.kind, rhs.kind) && plannerSameText(lhs.taskID, rhs.taskID)
            && plannerSameText(lhs.name, rhs.name)
            && plannerSameText(lhs.engineExecutable, rhs.engineExecutable)
            && plannerSameText(lhs.sourceRaw, rhs.sourceRaw)
            && plannerSameText(lhs.destinationRaw, rhs.destinationRaw)
            && lhs.options == rhs.options
    }
}

enum PlannerPathCheckValue: Sendable, Equatable {
    case checkedAtPlanning
}

enum PlannerEngineCheckValue: Sendable, Equatable {
    case notPerformed
}

struct PlannerPlanValue: Sendable, Equatable {
    let contractVersion: String
    let taskSnapshot: PlannerTaskValue
    let executable: String
    let runArguments: [String]
    let dryRunArguments: [String]
    let pathCheck: PlannerPathCheckValue
    let engineCheck: PlannerEngineCheckValue

    static func == (lhs: Self, rhs: Self) -> Bool {
        plannerSameText(lhs.contractVersion, rhs.contractVersion)
            && lhs.taskSnapshot == rhs.taskSnapshot
            && plannerSameText(lhs.executable, rhs.executable)
            && plannerSameTexts(lhs.runArguments, rhs.runArguments)
            && plannerSameTexts(lhs.dryRunArguments, rhs.dryRunArguments)
            && lhs.pathCheck == rhs.pathCheck && lhs.engineCheck == rhs.engineCheck
    }
}

enum PlannerFailureCode: String, Sendable, Equatable {
    case invalidJson = "invalid_json"
    case unsupportedSchema = "unsupported_schema"
    case invalidTask = "invalid_task"
    case invalidPath = "invalid_path"
    case unsafePathRelation = "unsafe_path_relation"
    case pathAccessDenied = "path_access_denied"
    case pathIo = "path_io"
}

enum PlannerFailureValue: Sendable, Equatable {
    case planning(code: PlannerFailureCode, field: String?, message: String, osCode: Int32?)
    case bridge(message: String)

    var field: String? {
        if case .planning(_, let field, _, _) = self { field } else { nil }
    }

    var explanation: String {
        switch self {
        case .bridge:
            "The bridge could not return a valid plan. Review the details."
        case .planning(let code, _, _, _):
            switch code {
            case .invalidPath: "Check the absolute local path."
            case .unsafePathRelation:
                "Source and destination must be separate, non-nested directories."
            case .pathAccessDenied: "Directory checks were denied. Review the path and permissions."
            case .pathIo: "Directory checks could not be completed."
            case .invalidTask: "Review the task values and select exactly one argument profile."
            case .invalidJson, .unsupportedSchema:
                "The task format or contract could not be accepted."
            }
        }
    }

    var details: String {
        switch self {
        case .bridge(let message):
            message
        case .planning(let code, let field, let message, let osCode):
            "Code: \(code.rawValue)\nField: \(field ?? "not provided")\nMessage: \(message)"
                + "\nOS code: \(osCode.map(String.init) ?? "not provided")"
        }
    }
}

enum PlannerOutcome: Sendable, Equatable {
    case success(PlannerPlanValue)
    case failure(PlannerFailureValue)
}

struct PlannerRequestToken: Sendable, Equatable {
    let requestSequence: UInt64
    let formRevision: UInt64
}

struct PlannerCompletion: Sendable, Equatable {
    let token: PlannerRequestToken
    let outcome: PlannerOutcome
}

enum PlannerPhase: Sendable, Equatable {
    case idle, generating, outOfDate, generated, failed, closed
}

struct PlannerViewState: Sendable {
    var form = PlannerFormValues()
    var formRevision: UInt64 = 1
    var requestSequence: UInt64 = 0
    var inFlight: PlannerRequestToken?
    var preview: PlannerPlanValue?
    var previewIsCurrent = false
    var failure: PlannerFailureValue?
    var phase = PlannerPhase.idle
    var windowIsOpen = true
    var acceptedResponseSequence: UInt64?

    var canGenerate: Bool {
        windowIsOpen && inFlight == nil && form.isComplete && requestSequence < UInt64.max
    }

    var canCopy: Bool {
        windowIsOpen && inFlight == nil && previewIsCurrent && preview != nil && failure == nil
    }
}

enum PlannerText {
    static func quoted(_ value: String) -> String {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.withoutEscapingSlashes]
        guard let data = try? encoder.encode(value) else { return "[unavailable]" }
        return String(decoding: data, as: UTF8.self)
    }

    static func copyJSON(executable: String, arguments: [String]) throws -> String {
        let data = try JSONSerialization.data(
            withJSONObject: ["executable": executable, "args": arguments],
            options: [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes])
        return String(decoding: data, as: UTF8.self)
    }
}
