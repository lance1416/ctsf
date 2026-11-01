import AppKit
import SwiftUI

@MainActor
struct PlannerPreviewView: View {
    @State private var coordinator = PlannerCoordinator(
        provider: PlannerWorker(operation: PlannerFFI.plan))
    @State private var snapshot = PlannerViewState()
    @State private var panelError: String?
    @State private var copiedMode: ArgumentMode?
    @FocusState private var focusedField: InputField?
    @AccessibilityFocusState private var errorFocused: Bool

    private enum InputField: Hashable {
        case engine, source, destination, options
    }

    private enum ArgumentMode: String, Sendable {
        case run = "Run"
        case dryRun = "Dry Run"
    }

    var body: some View {
        VStack(spacing: 0) {
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    KernelHealthView()
                    Divider()
                    Text("Local task plan").font(.title2)
                    Text(
                        "Check local directories and generate arguments. No rsync process will run."
                    )
                    form
                    if let panelError {
                        Text(panelError).foregroundStyle(.red).textSelection(.enabled)
                    }
                    if let failure = snapshot.failure {
                        failureView(failure)
                    }
                    if let plan = snapshot.preview {
                        preview(plan)
                    } else {
                        Text("No current invocation preview.").foregroundStyle(.secondary)
                    }
                }
                .padding(28)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            Divider()
            actionBar
        }
        .frame(minWidth: 680, minHeight: 600)
        .onAppear(perform: attach)
        .onDisappear {
            coordinator.onStateChange = nil
            coordinator.close()
        }
        .onChange(of: snapshot.acceptedResponseSequence) { _, _ in
            reportCompletion()
        }
        .task(id: copiedMode) {
            guard copiedMode != nil else { return }
            do {
                try await Task.sleep(for: .seconds(2))
                copiedMode = nil
            } catch {}
        }
    }

    private var form: some View {
        VStack(alignment: .leading, spacing: 16) {
            pathField(
                "Engine executable", keyPath: \.engineExecutable, focus: .engine,
                identifier: "planner-engine", errorField: "engineExecutable")
            Text("Path only. Engine existence, permissions and version are not checked.")
                .font(.caption).foregroundStyle(.secondary)

            pathField(
                "Source directory", keyPath: \.sourceRaw, focus: .source,
                identifier: "planner-source", errorField: "source.raw",
                choose: { chooseDirectory(source: true) })
            Text(sourceMeaning).font(.caption).foregroundStyle(.secondary)

            pathField(
                "Destination directory", keyPath: \.destinationRaw, focus: .destination,
                identifier: "planner-destination", errorField: "destination.raw",
                choose: { chooseDirectory(source: false) })
            Text("Choose an existing local test directory.")
                .font(.caption).foregroundStyle(.secondary)

            Text("Options").font(.headline)
            PlannerOptionPicker(selected: snapshot.form.option) { option in
                var form = coordinator.state.form
                form.option = option
                coordinator.updateForm(form)
            }
            .frame(height: 50)
            .accessibilityIdentifier("planner-options")
            .focusable()
            .focused($focusedField, equals: .options)
            Text(
                "Choose one argument profile. Engine support and metadata preservation are not verified."
            )
            .font(.caption).foregroundStyle(.secondary)
            if snapshot.failure?.field == "options" {
                Text(snapshot.failure?.explanation ?? "")
                    .font(.caption).foregroundStyle(.red)
            }
        }
    }

    private func pathField(
        _ title: String, keyPath: WritableKeyPath<PlannerFormValues, String>, focus: InputField,
        identifier: String, errorField: String, choose: (@MainActor () -> Void)? = nil
    ) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title).font(.headline)
            HStack {
                TextField(title, text: binding(keyPath))
                    .textFieldStyle(.roundedBorder)
                    .font(.system(.body, design: .monospaced))
                    .focused($focusedField, equals: focus)
                    .accessibilityIdentifier(identifier)
                    .help(
                        "Enter the complete local path without shell quotes. The raw value is preserved."
                    )
                if let choose {
                    Button(
                        focus == .source ? "Choose source…" : "Choose destination…", action: choose
                    )
                    .accessibilityIdentifier(
                        focus == .source ? "planner-choose-source" : "planner-choose-destination")
                }
            }
            if snapshot.failure?.field == errorField {
                Text(snapshot.failure?.explanation ?? "").font(.caption).foregroundStyle(.red)
            }
        }
    }

    private func binding(_ keyPath: WritableKeyPath<PlannerFormValues, String>) -> Binding<String> {
        Binding(
            get: { snapshot.form[keyPath: keyPath] },
            set: { value in
                var form = coordinator.state.form
                form[keyPath: keyPath] = value
                coordinator.updateForm(form)
            })
    }

    private var sourceMeaning: String {
        let raw = snapshot.form.sourceRaw
        if raw.isEmpty { return "Enter a source directory. A trailing / changes its meaning." }
        if raw == "/" || raw.hasSuffix("//") {
            return "Review the raw path and generated arguments."
        }
        return raw.hasSuffix("/")
            ? "Trailing /: copy directory contents."
            : "No trailing /: include the source directory itself."
    }

    private var actionBar: some View {
        HStack(spacing: 12) {
            if snapshot.inFlight != nil { ProgressView().controlSize(.small) }
            Text(status).font(.callout)
                .frame(maxWidth: .infinity, alignment: .leading)
                .accessibilityIdentifier("planner-status")
            Button("Generate plan") { coordinator.generate() }
                .keyboardShortcut("p", modifiers: [.command, .shift])
                .disabled(!snapshot.canGenerate)
                .accessibilityIdentifier("planner-generate")
        }
        .padding(20)
    }

    private var status: String {
        if snapshot.inFlight != nil {
            return snapshot.phase == .outOfDate
                ? "Generating plan… Inputs changed; this response will be discarded."
                : "Generating plan…"
        }
        if !snapshot.form.isComplete {
            if snapshot.form.engineExecutable.isEmpty { return "Enter an engine executable path." }
            if snapshot.form.sourceRaw.isEmpty { return "Enter a source directory." }
            if snapshot.form.destinationRaw.isEmpty { return "Enter a destination directory." }
            return "Select Recursive or Archive."
        }
        switch snapshot.phase {
        case .idle: return "No plan generated."
        case .generating: return "Generating plan…"
        case .outOfDate: return "Inputs changed. Generate a new plan."
        case .generated: return "Plan generated. No rsync process was run."
        case .failed: return "Could not generate a plan."
        case .closed: return "Window closed."
        }
    }

    private func failureView(_ failure: PlannerFailureValue) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Could not generate a plan").font(.headline)
            Text(failure.explanation)
            Text(failure.details).font(.system(.caption, design: .monospaced))
                .textSelection(.enabled)
        }
        .foregroundStyle(.red)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("planner-error")
        .accessibilityFocused($errorFocused)
    }

    private func preview(_ plan: PlannerPlanValue) -> some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Invocation preview").font(.title2)
            if !snapshot.previewIsCurrent {
                Text(
                    snapshot.inFlight != nil && snapshot.phase != .outOfDate
                        ? "Previous plan — generating a new plan."
                        : "Out of date — inputs changed. Generate a new plan."
                )
                .font(.headline)
            }
            Text(
                "Planner contract: \(plan.contractVersion) · revision \(plan.taskSnapshot.revision)"
            )
            Text("Directories checked when this plan was generated.")
            Text(
                "Directories may change. Transfer permissions and filesystem stability are not verified."
            )
            .font(.caption).foregroundStyle(.secondary)
            Text("Engine not checked.")
            Text("No files copied. No Dry Run performed.")
            Text("Executable (argv[0])").font(.headline)
            Text(PlannerText.quoted(plan.executable))
                .font(.system(.body, design: .monospaced)).textSelection(.enabled)
                .accessibilityIdentifier("planner-executable")
            Text("Arguments exclude argv[0].").font(.caption).foregroundStyle(.secondary)
            arguments(plan.runArguments, mode: .run)
            arguments(plan.dryRunArguments, mode: .dryRun)
        }
    }

    private func arguments(_ arguments: [String], mode: ArgumentMode) -> some View {
        GroupBox("\(mode.rawValue) argv (not executed)") {
            VStack(alignment: .leading, spacing: 10) {
                ForEach(arguments.indices, id: \.self) { index in
                    HStack(alignment: .top, spacing: 12) {
                        Text("argv[\(index + 1)]").foregroundStyle(.secondary)
                        Text(PlannerText.quoted(arguments[index]))
                            .textSelection(.enabled)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
                Button(copiedMode == mode ? "Copied" : "Copy \(mode.rawValue) plan") {
                    copy(mode)
                }
                .disabled(!snapshot.canCopy)
                .help("Copy executable and arguments as JSON, not a shell command.")
                .accessibilityIdentifier(mode == .run ? "planner-copy-run" : "planner-copy-dry-run")
            }
            .font(.system(.body, design: .monospaced))
            .padding(8)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .accessibilityIdentifier(mode == .run ? "planner-run-argv" : "planner-dry-run-argv")
    }

    private func attach() {
        if !coordinator.state.windowIsOpen {
            coordinator = PlannerCoordinator(provider: PlannerWorker(operation: PlannerFFI.plan))
        }
        coordinator.onStateChange = { snapshot = $0 }
        snapshot = coordinator.state
        focusedField = .engine
    }

    private func chooseDirectory(source: Bool) {
        guard let window = NSApp.keyWindow ?? NSApp.mainWindow else { return }
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.canCreateDirectories = false
        panel.resolvesAliases = false
        panel.prompt = source ? "Choose source" : "Choose destination"
        panel.beginSheetModal(for: window) { response in
            guard response == .OK, let url = panel.url else { return }
            let raw = url.withUnsafeFileSystemRepresentation { pointer -> String? in
                guard let pointer else { return nil }
                return String(validatingCString: pointer)
            }
            guard let raw else {
                panelError = "The selected directory cannot be represented as a UTF-8 path."
                return
            }
            var form = coordinator.state.form
            if source { form.sourceRaw = raw } else { form.destinationRaw = raw }
            panelError = nil
            coordinator.updateForm(form)
            focusedField = source ? .source : .destination
        }
    }

    private func copy(_ mode: ArgumentMode) {
        guard coordinator.state.canCopy, let plan = coordinator.state.preview else { return }
        do {
            let json = try PlannerText.copyJSON(
                executable: plan.executable,
                arguments: mode == .run ? plan.runArguments : plan.dryRunArguments)
            NSPasteboard.general.clearContents()
            if NSPasteboard.general.setString(json, forType: .string) { copiedMode = mode }
        } catch {
            panelError = "Could not encode the parameter review text: \(error)"
        }
    }

    private func reportCompletion() {
        guard snapshot.windowIsOpen, snapshot.acceptedResponseSequence != nil else { return }
        if let failure = snapshot.failure {
            switch failure.field {
            case "engineExecutable": focusedField = .engine
            case "source.raw": focusedField = .source
            case "destination.raw": focusedField = .destination
            case "options": focusedField = .options
            default: errorFocused = true
            }
        }
        NSAccessibility.post(
            element: NSApp as Any, notification: .announcementRequested,
            userInfo: [
                .announcement: status, .priority: NSAccessibilityPriorityLevel.medium.rawValue,
            ])
    }
}

@MainActor
private struct PlannerOptionPicker: NSViewRepresentable {
    let selected: PlannerOptionValue?
    let onSelect: @MainActor (PlannerOptionValue) -> Void

    func makeCoordinator() -> Coordinator { Coordinator(onSelect: onSelect) }

    func makeNSView(context: Context) -> NSStackView {
        let stack = NSStackView()
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 6
        for (index, option) in PlannerOptionValue.allCases.enumerated() {
            let button = NSButton(
                radioButtonWithTitle: option.label, target: context.coordinator,
                action: #selector(Coordinator.choose(_:)))
            button.tag = index
            button.setAccessibilityIdentifier("planner-option-\(option.rawValue)")
            stack.addArrangedSubview(button)
        }
        updateNSView(stack, context: context)
        return stack
    }

    func updateNSView(_ stack: NSStackView, context: Context) {
        context.coordinator.onSelect = onSelect
        for case let button as NSButton in stack.arrangedSubviews {
            button.state = PlannerOptionValue.allCases[button.tag] == selected ? .on : .off
        }
    }

    @MainActor
    final class Coordinator: NSObject {
        var onSelect: @MainActor (PlannerOptionValue) -> Void

        init(onSelect: @escaping @MainActor (PlannerOptionValue) -> Void) {
            self.onSelect = onSelect
        }

        @objc func choose(_ sender: NSButton) {
            onSelect(PlannerOptionValue.allCases[sender.tag])
        }
    }
}
