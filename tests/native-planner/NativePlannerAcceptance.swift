import Dispatch
import Foundation

@main
struct NativePlannerAcceptance {
    static func main() async {
        do {
            var arguments = Array(CommandLine.arguments.dropFirst())
            let typedOnly = arguments.first == "--typed-only"
            if typedOnly { arguments.removeFirst() }
            guard arguments.count == 2, arguments[0].hasPrefix("/") else {
                throw QAProblem(
                    "usage: NativePlannerAcceptance [--typed-only] /absolute/fixtures/native-planner expected-version"
                )
            }
            let assets = URL(fileURLWithPath: arguments[0], isDirectory: true)
            let version = arguments[1]
            let typed = try await Task.detached {
                try runTypedBridgeChecks(assets: assets, expectedVersion: version)
            }.value
            var checks = typed.checks
            if !typedOnly {
                checks += try await runProductionGateChecks(assets: assets).checks
            }
            let report = QAReport(
                scope: typedOnly
                    ? "real-typed-bridge-only"
                    : "real-typed-bridge-and-production-worker-coordinator",
                checks: checks)
            let encoder = JSONEncoder()
            encoder.outputFormatting = [.sortedKeys]
            print(String(decoding: try encoder.encode(report), as: UTF8.self))
            if report.failed { exit(1) }
        } catch {
            FileHandle.standardError.write(
                Data("native planner acceptance failed: \(error)\n".utf8))
            exit(1)
        }
    }
}
