import JojoCore
import SwiftUI

/// The primary window: the app shell around the review/lifecycle screens. The screens
/// that exist keep their state while another destination is shown. No terminal emulator.
struct MainView: View {
    @ObservedObject var model: AppModel
    let delegate: AppDelegate
    @State private var screen = Screen.dashboard
    @State private var range = DashboardRange.today
    @State private var newFeatureProject: String?
    @State private var exportingTranscript = false
    @State private var specificationMode = SpecificationMode.document

    var body: some View {
        Themed {
            AppShell(model: model, screen: $screen, showInspector: { delegate.showAuxiliary() },
                     controls: controls,
                     primary: screen == .dashboard ? AnyView(NewFeatureButton(newFeature: newFeature)) : nil,
                     heading: featureHeading ?? screenHeading) {
                VStack(alignment: .leading, spacing: 8) {
                    QuitBanner(model: model).padding(EdgeInsets(top: 20, leading: 20, bottom: 0, trailing: 20))
                    ZStack(alignment: .topLeading) {
                        Group {
                            if !MainView.built.contains(screen) { NotBuiltYet(screen: screen) }
                        }
                        .padding(.horizontal, 20)
                        // A redesigned screen fills the content area and sets its own padding.
                        shown(.dashboard) { DashboardScreen(model: model, visible: screen == .dashboard, range: range, open: { screen = $0 }) }
                        shown(.activity) { ActivityScreen(model: model, visible: screen == .activity, open: { screen = $0 }) }
                        shown(.agentRuns) { AgentRunsScreen(model: model, visible: screen == .agentRuns, open: { screen = $0 }) }
                        shown(.approvalInbox) {
                            ApprovalInboxScreen(model: model, visible: screen == .approvalInbox, open: { screen = $0 },
                                                compare: { document in
                                                    specificationMode = .compare
                                                    model.workspace.loadReview(document)
                                                    screen = .specifications
                                                })
                        }
                        shown(.featureChat) {
                            FeatureChatScreen(model: model, visible: screen == .featureChat, project: $newFeatureProject,
                                              exporting: $exportingTranscript, open: { screen = $0 })
                        }
                        shown(.executionPlan) { ExecutionPlanScreen(model: model, visible: screen == .executionPlan, open: { screen = $0 }) }
                        shown(.recoveryCenter) { RecoveryCenterScreen(model: model) }
                        shown(.settings) { SettingsScreen(model: model, visible: screen == .settings) }
                        shown(.sourceControl) { SourceControlScreen(model: model, open: { screen = $0 }) }
                        shown(.specifications) {
                            SpecificationsScreen(model: model, visible: screen == .specifications, mode: $specificationMode,
                                                 open: { screen = $0 })
                        }
                        shown(.verification) { VerificationScreen(model: model, open: { screen = $0 }) }
                        shown(.projects) { ProjectsScreen(model: model, visible: screen == .projects) }
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                    if !model.workspace.status.isEmpty {
                        Text(model.workspace.status).font(.callout).foregroundStyle(.secondary).textSelection(.enabled)
                            .padding(.horizontal, 20)
                    }
                }
                .padding(.bottom, 28)
            }
            .overlay { QuitOverlay(model: model) }
        }
        .frame(minWidth: 900, minHeight: 620)
    }

    /// Destinations that have a screen today; the others show `NotBuiltYet`.
    static let built: Set<Screen> = [.activity, .agentRuns, .approvalInbox, .dashboard, .recoveryCenter, .executionPlan,
                                     .featureChat, .projects, .settings, .sourceControl, .specifications, .verification]

    /// Feature Chat holding no feature is New Feature ("Workspace · Crew · New Feature");
    /// holding one, its code and name ("FT-3 Leaderboard API", "Crew · Discovery rev 2").
    private var featureHeading: (title: String, subtitle: String)? {
        guard screen == .featureChat else { return nil }
        let workspace = model.workspace
        guard let feature = workspace.chatFeature else {
            let projects = workspace.projects?.projects ?? []
            let project = projects.first { $0.id == newFeatureProject } ?? projects.first
            return ("New Feature", (["Workspace", project?.name].compactMap { $0 } + ["New Feature"]).joined(separator: " · "))
        }
        return FeatureChatState.heading(feature: feature, summary: workspace.features?.features.first { $0.id == feature },
                                        name: workspace.chatIndex?.features.first { $0.id == feature }?.name,
                                        turns: workspace.chat?.feature == feature ? workspace.chat?.turns ?? [] : [])
    }

    /// The feature screens of step 4 name the held feature in their sub line
    /// ("Sample › FT-3 Slugify › Specifications"); Specifications is titled by its document.
    private var screenHeading: (title: String, subtitle: String)? {
        let workspace = model.workspace
        let scope = FeatureScope(workspace: workspace)
        switch screen {
        case .specifications:
            let state = SpecificationsState(connection: model.client.connection, admission: workspace.admission,
                                            scope: scope, documents: workspace.documents, detail: workspace.document)
            return (state.title, scope.place("Specifications"))
        case .approvalInbox:
            let pending = workspace.documents?.pending ?? 0
            return (screen.title, "\(pending) pending · no approve-all")
        case .executionPlan, .verification:
            return (screen.title, scope.place(nil))
        case .agentRuns:
            return (screen.title, scope.place("attempts"))
        case .sourceControl:
            return (screen.title, scope.place(workspace.attemptDetail?.key))
        default:
            return nil
        }
    }

    /// The shown screen's header controls: the Dashboard's time range, a chat's spike and
    /// export buttons, a feature screen's buttons.
    private var controls: AnyView? {
        if screen == .specifications {
            return AnyView(HStack(spacing: 10) {
                SegmentedCapsule(selection: $specificationMode, label: "View")
                HeaderButtons(buttons: [HeaderButton(title: "Approval History") { screen = .approvalInbox },
                                        HeaderButton(title: "Affected Tasks") { screen = .executionPlan }])
            })
        }
        if screen == .approvalInbox {
            return AnyView(HeaderButtons(buttons: [HeaderButton(title: "Approval History") { screen = .activity }]))
        }
        if screen == .dashboard { return AnyView(SegmentedCapsule(selection: $range, label: "Time range")) }
        if screen == .featureChat, model.workspace.chatFeature != nil {
            return AnyView(ChatHeaderActions(export: { exportingTranscript = true }))
        }
        return nil
    }

    /// New Feature is Feature Chat holding no feature.
    private func newFeature() {
        model.workspace.openChat(nil)
        screen = .featureChat
    }

    private func shown(_ target: Screen, @ViewBuilder _ content: () -> some View) -> some View {
        content()
            .opacity(screen == target ? 1 : 0)
            .allowsHitTesting(screen == target)
            .accessibilityHidden(screen != target)
    }
}

/// Failures stay visible after the dialog is gone; they never read as a clean shutdown.
struct QuitBanner: View {
    @ObservedObject var model: AppModel

    var body: some View {
        if case .failed(let failure) = model.coordinator.phase {
            VStack(alignment: .leading, spacing: 6) {
                Text("Jojo did not close cleanly").font(.headline)
                Text(QuitText.failure(failure)).textSelection(.enabled)
                HStack {
                    if case .unsettledCommands = failure {
                        Button("Quit without these commands") {
                            model.coordinator.acknowledgeUnsettled()
                            model.quit(.quitCommand)
                        }
                    } else if model.client.connection.helperMayBeAlive {
                        Button("Disconnect and Quit (unclean)…") { model.coordinator.disconnectAndQuit() }
                    }
                }
            }
            .padding(10)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color.red.opacity(0.12), in: RoundedRectangle(cornerRadius: 6))
        }
    }
}

struct QuitOverlay: View {
    @ObservedObject var model: AppModel

    var body: some View {
        if let content = panel {
            ZStack {
                Color.black.opacity(0.25).ignoresSafeArea()
                VStack(alignment: .leading, spacing: 12) { content }
                    .padding(20)
                    .frame(maxWidth: 460, alignment: .leading)
                    .background(.background, in: RoundedRectangle(cornerRadius: 10))
                    .shadow(radius: 12)
            }
        }
    }

    private var panel: AnyView? {
        switch model.coordinator.phase {
        case .confirmRequired(_, let active):
            // Cancel is the default action; confirmation is never preselected or inferred.
            return AnyView(Group {
                Text("Stop active work and quit?").font(.headline)
                Text("\(active) active agent operation(s) — such as Claude Code or Codex runs — will be interrupted for shutdown. Worktrees, task state, logs, checkpoints and recovery evidence are preserved, and interrupted work can be inspected and safely recovered after reopening. Interruption is not task cancellation.")
                if active == 0 {
                    Text("Active work has finished while this dialog was open. Quitting still requires your confirmation.")
                        .foregroundStyle(.secondary)
                }
                HStack {
                    Spacer()
                    Button("Cancel") { model.coordinator.cancel() }.keyboardShortcut(.defaultAction)
                    Button("Confirm Quit", role: .destructive) { model.coordinator.confirm() }
                }
            })
        case .reconciling(let n):
            return AnyView(Text("Confirming \(n) pending command(s) were saved before closing…"))
        case .preparing:
            return AnyView(Text("Checking for active work…"))
        case .finalizing:
            return AnyView(Text("No active work. Verifying clean shutdown…"))
        case .confirming:
            return AnyView(Text("Requesting coordinated shutdown…"))
        case .cancelling:
            return AnyView(Text("Cancelling quit; running work continues…"))
        case .stopping(_, let active):
            return AnyView(Group {
                Text("Stopping active work…").font(.headline)
                ProgressView()
                Text(active.map { "\($0) operation(s) still active." } ?? "Waiting for the helper's stop report.")
                if let at = model.coordinator.confirmedAt {
                    TimelineView(.periodic(from: at, by: 1)) { context in
                        Text("Elapsed \(Int(context.date.timeIntervalSince(at))) s. Jojo quits only after a verified shutdown receipt and helper exit.")
                            .foregroundStyle(.secondary)
                    }
                }
            })
        case .awaitingExit:
            return AnyView(Text("Shutdown verified. Waiting for the helper to exit…"))
        case .disconnectRequired(let reason):
            return AnyView(Group {
                Text("Jojo cannot verify a clean shutdown").font(.headline)
                Text(reason)
                Text("Quitting disconnects the helper. It will run its emergency stop for any active work, and this session will be recorded as unclean. Recovery inspection runs on the next launch.")
                HStack {
                    Spacer()
                    Button("Stay in Jojo") { model.coordinator.stay() }.keyboardShortcut(.defaultAction)
                    Button("Disconnect and Quit", role: .destructive) { model.coordinator.disconnectAndQuit() }
                }
            })
        case .idle, .clean, .failed:
            return nil
        }
    }
}

enum QuitText {
    static func failure(_ f: QuitFailure) -> String {
        switch f {
        case .unsettledCommands(let ids):
            return "Could not confirm that \(ids.count) pending command(s) were saved (\(ids.joined(separator: ", "))). They will not be resent."
        case .closeRejected(let c): return "The helper rejected the close request: \(c). Quit again to retry."
        case .shutdownFailed(let s): return "Shutdown failed or is unconfirmed: \(s). Affected work stays held for recovery; nothing is claimed clean."
        case .invalidProof(let s): return "The shutdown receipt was not valid proof (\(s))."
        case .helperEnded(let status, let afterProof):
            return "The helper exited with status \(status)\(afterProof ? " after its ready receipt" : " without a shutdown receipt"). State is unknown until reopen inspection."
        case .exitTimeout: return "The helper did not exit after its ready receipt."
        case .channelLost(let r): return "The helper channel was lost (\(r)). State is unknown; nothing is assumed stopped."
        case .unexpectedState(let s): return "Unexpected close state: \(s). Quit again to retry."
        }
    }
}

/// Auxiliary inspector window: closing it never quits Jojo (U02).
struct InspectorView: View {
    @ObservedObject var model: AppModel

    var body: some View {
        VStack(alignment: .leading) {
            Text("Inspector").font(.headline)
            Text("Quit state: \(model.coordinator.phase.name)")
            Text("Snapshot cursor: \(model.workspace.snapshotSeq.map(String.init) ?? "none")")
            Text("Recovery digest: \(model.workspace.recoveryDigest)").textSelection(.enabled)
            Spacer()
        }
        .padding(12)
    }
}
