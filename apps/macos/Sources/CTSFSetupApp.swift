import SwiftUI

@main
@MainActor
struct CTSFSetupApp: App {
    var body: some Scene {
        Window("CTSF Setup", id: "setup") {
            PlannerPreviewView()
        }
        .defaultSize(width: 820, height: 820)
    }
}

@MainActor
struct KernelHealthView: View {
    // setup.v1 queries are stateless and perform no I/O.
    private let info = kernelInfo()
    private let report = health()

    private var statusLabel: String {
        switch report.status {
        case .ready:
            "Ready"
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 20) {
            Text("CTSF")
                .font(.largeTitle.bold())

            Text("Kernel connection")
                .font(.title3)

            GroupBox {
                VStack(spacing: 12) {
                    LabeledContent("Kernel version", value: info.kernelVersion)
                        .accessibilityIdentifier("kernel-version")
                    LabeledContent("Contract", value: info.contractVersion)
                        .accessibilityIdentifier("kernel-contract")
                    LabeledContent("Library health", value: statusLabel)
                        .accessibilityIdentifier("kernel-health")
                }
                .textSelection(.enabled)
                .padding(8)
            }

            Text("Local library connection check")
                .font(.footnote)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}
