import Darwin
import Dispatch
import Foundation

struct QACheck: Codable, Sendable {
    let id: String
    let status: String
    let detail: String
}

struct QAReport: Codable, Sendable {
    let scope: String
    let checks: [QACheck]
    let status: String

    init(scope: String, checks: [QACheck]) {
        self.scope = scope
        self.checks = checks
        status =
            checks.contains { $0.status == "FAIL" }
            ? "FAIL"
            : checks.contains { $0.status == "UNVERIFIED" } ? "PASS_WITH_LIMITATIONS" : "PASS"
    }

    var failed: Bool { checks.contains { $0.status == "FAIL" } }
}

struct QAProblem: Error, CustomStringConvertible {
    let description: String
    init(_ description: String) { self.description = description }
}

func qaRequire(_ condition: Bool, _ message: String) throws {
    if !condition { throw QAProblem(message) }
}

func qaBytesEqual(_ actual: String, _ expected: String, _ field: String) throws {
    try qaRequire(Array(actual.utf8) == Array(expected.utf8), "UTF-8 mismatch: \(field)")
}

func qaTokensEqual(_ actual: [String], _ expected: [String], _ field: String) throws {
    try qaRequire(actual.count == expected.count, "token count mismatch: \(field)")
    for index in actual.indices {
        try qaBytesEqual(actual[index], expected[index], "\(field)[\(index)]")
    }
}

func qaTaskEqual(_ actual: PlannerTask, _ expected: PlannerTask) throws {
    try qaRequire(actual.schemaVersion == expected.schemaVersion, "schemaVersion mismatch")
    try qaRequire(actual.revision == expected.revision, "UInt64 revision mismatch")
    try qaBytesEqual(actual.kind, expected.kind, "kind")
    try qaBytesEqual(actual.taskId, expected.taskId, "taskId")
    try qaBytesEqual(actual.name, expected.name, "name")
    try qaBytesEqual(actual.engineExecutable, expected.engineExecutable, "engineExecutable")
    try qaBytesEqual(actual.source.raw, expected.source.raw, "source.raw")
    try qaBytesEqual(actual.destination.raw, expected.destination.raw, "destination.raw")
    try qaRequire(actual.options == expected.options, "typed options mismatch")
}

final class QAChecks {
    private(set) var values: [QACheck] = []

    func run(_ id: String, _ operation: () throws -> Void) {
        do {
            try operation()
            values.append(QACheck(id: id, status: "PASS", detail: ""))
        } catch {
            values.append(QACheck(id: id, status: "FAIL", detail: String(describing: error)))
        }
    }

    func unverified(_ id: String, _ reason: String) {
        values.append(QACheck(id: id, status: "UNVERIFIED", detail: reason))
    }
}

final class NativePlannerFixture {
    let root: URL
    let assets: URL
    let source: String
    let destination: String
    let missingEngine: String
    let markerEngine: URL
    private let manager = FileManager.default

    init(assets: URL) throws {
        self.assets = assets
        root = manager.temporaryDirectory.appendingPathComponent(
            "ctsf-native-qa-\(UUID().uuidString)")
        source = root.appendingPathComponent("源 空格 e\u{301} : $HOME * ? [x] ' \"").path + "/"
        destination = root.appendingPathComponent("目标 目录").path + "/"
        missingEngine = root.appendingPathComponent("not-installed-engine").path
        markerEngine = root.appendingPathComponent("marker engine.sh")
        try manager.createDirectory(atPath: source, withIntermediateDirectories: true)
        try manager.createDirectory(atPath: destination, withIntermediateDirectories: true)
        try Data("read-only source 中文\n".utf8).write(
            to: URL(fileURLWithPath: source).appendingPathComponent("payload"))
        try Data([0, 1, 2, 127, 128, 255]).write(
            to: URL(fileURLWithPath: destination).appendingPathComponent("existing"))
        try manager.copyItem(
            at: assets.appendingPathComponent("engine-marker.sh"), to: markerEngine)
        try manager.setAttributes([.posixPermissions: 0o755], ofItemAtPath: markerEngine.path)
    }

    func remove() { try? manager.removeItem(at: root) }

    func task(option: PlannerTaskOption = .recursive, revision: UInt64 = .max) -> PlannerTask {
        PlannerTask(
            schemaVersion: 1, kind: "rsync.task.pot", taskId: "qa-native-plan",
            revision: revision, name: "QA 中文 e\u{301}", engineExecutable: missingEngine,
            source: PlannerLocalPath(raw: source), destination: PlannerLocalPath(raw: destination),
            options: [option]
        )
    }

    func taskJSON() throws -> String {
        var text = try String(
            contentsOf: assets.appendingPathComponent("task.json"), encoding: .utf8)
        for (token, value) in [
            ("@SOURCE@", source), ("@DESTINATION@", destination), ("@ENGINE@", missingEngine),
        ] {
            let quoted = String(decoding: try JSONEncoder().encode(value), as: UTF8.self)
            text = text.replacingOccurrences(of: "\"\(token)\"", with: quoted)
        }
        return text
    }

    func expectedTokens(profile: String, key: String, task: PlannerTask) throws -> [String] {
        let data = try Data(contentsOf: assets.appendingPathComponent("argv-oracle.json"))
        guard
            let profiles = try JSONSerialization.jsonObject(with: data)
                as? [String: [String: [String]]],
            let tokens = profiles[profile]?[key]
        else { throw QAProblem("missing handwritten argv oracle") }
        return tokens.map { token in
            switch token {
            case "@SOURCE@": task.source.raw
            case "@DESTINATION@": task.destination.raw
            default: token
            }
        }
    }

    func checkPlan(_ plan: PlannerInvocationPlan, task: PlannerTask, profile: String) throws {
        try qaBytesEqual(plan.contractVersion, "planner.v1", "contractVersion")
        try qaTaskEqual(plan.taskSnapshot, task)
        try qaBytesEqual(plan.executable, task.engineExecutable, "executable")
        try qaTokensEqual(
            plan.runArgv, expectedTokens(profile: profile, key: "run", task: task), "runArgv")
        try qaTokensEqual(
            plan.dryRunArgv, expectedTokens(profile: profile, key: "dryRun", task: task),
            "dryRunArgv")
        try qaRequire(plan.pathCheck == .checkedAtPlanning, "pathCheck mismatch")
        try qaRequire(plan.engineCheck == .notPerformed, "engineCheck must remain not_performed")
    }

    func manifest() throws -> [String: String] {
        var result: [String: String] = [:]
        func visit(_ url: URL) throws {
            let attributes = try manager.attributesOfItem(atPath: url.path)
            let relative = String(url.path.dropFirst(root.path.count))
            let type = attributes[.type] as? FileAttributeType
            let mode = String(describing: attributes[.posixPermissions])
            let modified = String(describing: attributes[.modificationDate])
            if type == .typeSymbolicLink {
                result[relative] =
                    "link|\(mode)|\(modified)|\(try manager.destinationOfSymbolicLink(atPath: url.path))"
            } else if type == .typeDirectory {
                result[relative] = "directory|\(mode)|\(modified)"
                for child in try manager.contentsOfDirectory(
                    at: url, includingPropertiesForKeys: nil
                ).sorted(by: { $0.path < $1.path }) {
                    try visit(child)
                }
            } else {
                result[relative] =
                    "file|\(mode)|\(modified)|\(try Data(contentsOf: url).base64EncodedString())"
            }
        }
        try visit(root)
        return result
    }
}

enum QAExpectedOSCode {
    case none
    case some(Int32)
    case present
}

func expectPlanningError(
    code expectedCode: PlannerErrorCode, field expectedField: String?,
    osCode expectedOS: QAExpectedOSCode,
    _ operation: () throws -> Void
) throws {
    do {
        try operation()
    } catch let error as PlannerBridgeError {
        switch error {
        case .Planning(let code, let field, let message, let osCode):
            try qaRequire(code == expectedCode, "typed error code mismatch: \(code)")
            if let expectedField {
                guard let field else { throw QAProblem("typed error field became nil") }
                try qaBytesEqual(field, expectedField, "error.field")
            } else {
                try qaRequire(field == nil, "typed None field was not preserved")
            }
            try qaRequire(!message.isEmpty, "typed error message missing")
            switch expectedOS {
            case .none: try qaRequire(osCode == nil, "typed None osCode was not preserved")
            case .some(let expected):
                try qaRequire(osCode == expected, "typed Some(Int32) osCode mismatch")
            case .present: try qaRequire(osCode != nil, "typed Some(Int32) osCode was lost")
            }
            return
        }
    }
    throw QAProblem("expected typed PlannerBridgeError")
}

func observedPOSIXCode(_ operation: () throws -> Void) throws -> Int32 {
    do { try operation() } catch {
        var current = error as NSError
        for _ in 0..<8 {
            if current.domain == NSPOSIXErrorDomain, let code = Int32(exactly: current.code) {
                return code
            }
            guard let underlying = current.userInfo[NSUnderlyingErrorKey] as? NSError else { break }
            current = underlying
        }
        throw QAProblem("independent filesystem failure did not expose a POSIX errno")
    }
    throw QAProblem("filesystem error precondition did not occur")
}

func runTypedBridgeChecks(assets: URL, expectedVersion: String) throws -> QAReport {
    let fixture = try NativePlannerFixture(assets: assets)
    defer { fixture.remove() }
    let checks = QAChecks()
    let originalManifest = try fixture.manifest()

    checks.run("setup-compatibility") {
        let info = kernelInfo()
        try qaBytesEqual(info.kernelVersion, expectedVersion, "kernelVersion")
        try qaBytesEqual(info.contractVersion, "setup.v1", "setup contract")
        try qaRequire(health().status == .ready, "legacy health changed")
    }
    checks.run("parse-recursive-utf8-u64-max") {
        let parsed = try parseTaskJson(json: fixture.taskJSON())
        let expected = fixture.task()
        try qaTaskEqual(parsed, expected)
        let plan = try buildLocalPlan(task: parsed)
        try fixture.checkPlan(plan, task: expected, profile: "recursive")
    }
    checks.run("typed-archive-over-int64-and-snapshot") {
        var input = fixture.task(option: .archive, revision: UInt64(Int64.max) + 17)
        let expected = input
        let plan = try buildLocalPlan(task: input)
        input.source = PlannerLocalPath(raw: "/changed-after-call")
        input.options = [.recursive]
        try fixture.checkPlan(plan, task: expected, profile: "archive")
    }
    checks.run("deterministic-plan") {
        let task = fixture.task()
        try qaRequire(
            try buildLocalPlan(task: task) == buildLocalPlan(task: task),
            "plan was not deterministic")
    }
    checks.run("existing-marker-engine-not-inspected-or-executed") {
        var task = fixture.task()
        task.engineExecutable = fixture.markerEngine.path
        try fixture.checkPlan(buildLocalPlan(task: task), task: task, profile: "recursive")
    }
    checks.run("parse-invalid-json-none-field-none-os") {
        try expectPlanningError(code: .invalidJson, field: nil, osCode: .none) {
            _ = try parseTaskJson(json: "{\"schemaVersion\":]}")
        }
    }
    for (id, find, replacement) in [
        ("parse-unknown-field", "\"taskId\":", "\"extra\":true,\"taskId\":"),
        ("parse-duplicate-key", "\"taskId\":", "\"taskId\":\"first\",\"taskId\":"),
        ("parse-recursive-object-kind", "\"kind\": \"recursive\"", "\"kind\":{\"recursive\":null}"),
        ("parse-archive-object-kind", "\"kind\": \"recursive\"", "\"kind\":{\"archive\":null}"),
    ] {
        checks.run(id) {
            let json = try fixture.taskJSON()
            try qaRequire(json.contains(find), "invalid JSON fixture replacement missed")
            try expectPlanningError(code: .invalidTask, field: nil, osCode: .none) {
                _ = try parseTaskJson(json: json.replacingOccurrences(of: find, with: replacement))
            }
        }
    }
    checks.run("typed-unsupported-schema-some-field-none-os") {
        var task = fixture.task()
        task.schemaVersion = 2
        try expectPlanningError(code: .unsupportedSchema, field: "schemaVersion", osCode: .none) {
            _ = try buildLocalPlan(task: task)
        }
    }
    checks.run("typed-invalid-options") {
        var task = fixture.task()
        task.options = []
        try expectPlanningError(code: .invalidTask, field: "options", osCode: .none) {
            _ = try buildLocalPlan(task: task)
        }
    }
    checks.run("typed-invalid-raw-path") {
        var task = fixture.task()
        task.source = PlannerLocalPath(raw: "relative/source")
        try expectPlanningError(code: .invalidPath, field: "source.raw", osCode: .none) {
            _ = try buildLocalPlan(task: task)
        }
    }
    checks.run("typed-unsafe-directory-relation") {
        var task = fixture.task()
        task.destination = task.source
        try expectPlanningError(code: .unsafePathRelation, field: "destination.raw", osCode: .none)
        {
            _ = try buildLocalPlan(task: task)
        }
    }
    checks.run("typed-missing-directory-some-os") {
        var task = fixture.task()
        let missing = fixture.root.appendingPathComponent("missing-source").path
        task.source = PlannerLocalPath(raw: missing)
        let errno = try observedPOSIXCode {
            _ = try FileManager.default.attributesOfItem(atPath: missing)
        }
        try expectPlanningError(code: .invalidPath, field: "source.raw", osCode: .some(errno)) {
            _ = try buildLocalPlan(task: task)
        }
    }
    checks.run("typed-path-io-some-os") {
        let loop = fixture.root.appendingPathComponent("loop")
        try FileManager.default.createSymbolicLink(
            atPath: loop.path, withDestinationPath: loop.path)
        defer { try? FileManager.default.removeItem(at: loop) }
        var task = fixture.task()
        task.source = PlannerLocalPath(raw: loop.path)
        let errno = try observedPOSIXCode { _ = try Data(contentsOf: loop) }
        try expectPlanningError(code: .pathIo, field: "source.raw", osCode: .some(errno)) {
            _ = try buildLocalPlan(task: task)
        }
    }
    let protected = fixture.root.appendingPathComponent("protected")
    let inner = protected.appendingPathComponent("inner")
    try FileManager.default.createDirectory(at: inner, withIntermediateDirectories: true)
    try FileManager.default.setAttributes([.posixPermissions: 0], ofItemAtPath: protected.path)
    let preflight = inner.path.withCString { path -> (Int32, Int32) in
        var metadata = stat()
        let result = Darwin.fstatat(AT_FDCWD, path, &metadata, 0)
        return (result, errno)
    }
    if preflight.0 == -1 && preflight.1 == EACCES {
        checks.run("typed-permission-denied-some-os") {
            var task = fixture.task()
            task.source = PlannerLocalPath(raw: inner.path)
            try expectPlanningError(
                code: .pathAccessDenied, field: "source.raw", osCode: .some(preflight.1)
            ) {
                _ = try buildLocalPlan(task: task)
            }
        }
    } else {
        checks.unverified(
            "typed-permission-denied-some-os",
            "fstatat precondition: result=\(preflight.0), errno=\(preflight.1); expected actual EACCES"
        )
    }
    try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: protected.path)
    try FileManager.default.removeItem(at: protected)
    checks.run("fixture-content-and-engine-marker-unchanged") {
        // Harness-created/removed entries can update the root mtime; compare the contents' stable state.
        var before = originalManifest
        var after = try fixture.manifest()
        before.removeValue(forKey: "")
        after.removeValue(forKey: "")
        try qaRequire(before == after, "fixture entries/content/modes changed")
        try qaRequire(
            !FileManager.default.fileExists(atPath: fixture.markerEngine.path + ".executed"),
            "engine script ran")
    }
    return QAReport(scope: "real-typed-bridge-only", checks: checks.values)
}
