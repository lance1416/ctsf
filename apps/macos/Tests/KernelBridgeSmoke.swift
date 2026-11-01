@main
struct KernelBridgeSmoke {
    static func main() {
        precondition(CommandLine.arguments.count == 2, "Pass the expected kernel package version")
        let expectedVersion = CommandLine.arguments[1]
        let info = kernelInfo()
        let report = health()

        precondition(
            info.kernelVersion == expectedVersion, "Kernel version did not cross the bridge")
        precondition(info.contractVersion == "setup.v1", "Unexpected setup contract")
        precondition(report.status == .ready, "Kernel library is not ready")

        for _ in 0..<100 {
            precondition(kernelInfo() == info, "Kernel info changed across repeated bridge calls")
            precondition(health() == report, "Health changed across repeated bridge calls")
        }

        print(
            "ctsf-swift-bridge version=\(info.kernelVersion) "
                + "contract=\(info.contractVersion) health=\(report.status) repeated=100"
        )
    }
}
