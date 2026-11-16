import JojoCore
import SwiftUI

/// The primary window: the app shell around the review/lifecycle screens. The five screens
/// that exist keep their state while another destination is shown. No terminal emulator.
struct MainView: View {
    @ObservedObject var model: AppModel
    let delegate: AppDelegate
    @State private var subject = ""
    @State private var screen = Screen.agentRuns

    var body: some View {
        Themed {
            AppShell(model: model, screen: $screen, showInspector: { delegate.showAuxiliary() }) {
                VStack(alignment: .leading, spacing: 8) {
                    QuitBanner(model: model).padding(EdgeInsets(top: 20, leading: 20, bottom: 0, trailing: 20))
                    ZStack(alignment: .topLeading) {
                        Group {
                            shown(.agentRuns) { RunsView(model: model) }
                            shown(.approvalInbox) { ReviewView(model: model, subject: $subject) }
                            shown(.executionPlan) { StartView(model: model) }
                            shown(.verification) { CandidateView(model: model) }
                            if !MainView.built.contains(screen) { NotBuiltYet(screen: screen) }
                        }
                        .padding(.horizontal, 20)
                        // A redesigned screen fills the content area and sets its own padding.
                        shown(.recoveryCenter) { RecoveryCenterScreen(model: model) }
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
    static let built: Set<Screen> = [.agentRuns, .approvalInbox, .recoveryCenter, .executionPlan, .verification]

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

struct RunsView: View {
    @ObservedObject var model: AppModel

    var body: some View {
        List(model.workspace.rows) { row in
            HStack(alignment: .top) {
                VStack(alignment: .leading) {
                    Text("\(row.kind) \(row.subject)").font(.system(.body, design: .monospaced))
                    Text(row.value + (row.detailComplete ? "" : " … (partial)")).font(.caption).foregroundStyle(.secondary)
                }
                Spacer()
                if row.kind == "attempt" {
                    Button("Cancel Task") { model.workspace.cancelAttempt(row.subject) }
                }
            }
        }
        .overlay { if model.workspace.rows.isEmpty { Text("No runs or artifacts.").foregroundStyle(.secondary) } }
    }
}

struct ReviewView: View {
    @ObservedObject var model: AppModel
    @Binding var subject: String

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                TextField("Artifact identity (32 hex)", text: $subject).font(.system(.body, design: .monospaced))
                Button("Load Review") { model.workspace.loadReview(subject) }
            }
            if let b = model.workspace.review {
                Grid(alignment: .leading) {
                    GridRow { Text("Subject"); Text(b.subject) }
                    GridRow { Text("Revision"); Text("\(b.revision)") }
                    GridRow { Text("Hash"); Text(b.hash) }
                    GridRow { Text("Content"); Text(b.contentHash) }
                    GridRow { Text("Displayed"); Text(b.displayedHash) }
                    GridRow { Text("Context"); Text(b.contextHash) }
                    GridRow { Text("Upstream"); Text(b.upstream.map(String.init).joined(separator: ", ")) }
                }
                .font(.system(.caption, design: .monospaced))
                .textSelection(.enabled)
                HStack {
                    ForEach(DecisionAction.allCases, id: \.self) { action in
                        Button(action.rawValue) { model.workspace.decide(action) }
                    }
                }
                .disabled(model.workspace.reviewStale)
            }
            Spacer()
        }
        .padding(8)
    }
}

/// One human Start for one approved plan step. It names identities only; what is launched
/// is fixed by the helper's configuration and decided by the helper.
struct StartView: View {
    @ObservedObject var model: AppModel
    @State private var subjects = StartSubjects()

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Start runs one approved plan step in a Jojo-owned worktree. The helper refuses it unless admission is open, every approval is current and no earlier work is on hold.")
                .foregroundStyle(.secondary)
            Grid(alignment: .leading) {
                field("PRD", $subjects.prd)
                field("ADR", $subjects.adr)
                field("Plan", $subjects.plan)
                field("Validation plan", $subjects.validation)
                field("Step", $subjects.step)
            }
            Button("Start") { model.workspace.start(subjects) }
                .disabled(model.workspace.commandPending)
            Spacer()
        }
        .padding(8)
    }

    private func field(_ name: String, _ value: Binding<String>) -> some View {
        GridRow {
            Text(name)
            TextField("identity (32 hex)", text: value).font(.system(.body, design: .monospaced))
        }
    }
}

/// P2-03 result review: agent output, changed files, diff, write-scope verdict, the host
/// validation result and the human decision. PASS is shown only for the helper's recorded
/// host validation; what the provider reported is labelled as the provider's own claim.
struct CandidateView: View {
    @ObservedObject var model: AppModel
    @State private var attempt = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                TextField("Attempt identity (32 hex)", text: $attempt).font(.system(.body, design: .monospaced))
                Button("Load") { model.workspace.loadCandidate(attempt) }
            }
            if let c = model.workspace.candidate {
                ScrollView {
                    VStack(alignment: .leading, spacing: 10) {
                        result(c)
                        validation(c)
                        files
                        text("Git diff (base → sealed candidate)", model.workspace.diff)
                        text("Agent output and logs", model.workspace.agentLog.joined(separator: "\n"))
                        text("Validation report", model.workspace.validationReport)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                HStack {
                    Button("Run Validation") { model.workspace.validateCandidate() }
                        .disabled(c.candidate == nil || c.decision != "NONE")
                    Button("Accept Candidate") { model.workspace.decideCandidate(.Accept) }
                        .disabled(!c.canAccept)
                    Button("Reject Candidate", role: .destructive) { model.workspace.decideCandidate(.Reject) }
                        .disabled(c.candidate == nil || c.decision != "NONE")
                    Spacer()
                    Text("Accepting records your decision only. Nothing is merged, pushed or published.")
                        .font(.caption).foregroundStyle(.secondary)
                }
                .disabled(model.workspace.commandPending)
            } else {
                Text("Load a finished attempt to review its candidate.").foregroundStyle(.secondary)
            }
            Spacer(minLength: 0)
        }
        .padding(8)
    }

    private func result(_ c: CandidateReview) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text("Run").font(.headline)
            if c.sealed {
                Text("Outcome: \(c.outcome) — \(c.reason)")
                Text("Provider reported: \(c.providerTerminal.isEmpty ? "nothing" : c.providerTerminal) (the provider's own statement; not a validation result)")
                    .foregroundStyle(.secondary)
                Text("Sealed candidate: \(c.candidate ?? "none")").font(.system(.caption, design: .monospaced))
                Text("Decision: \(c.decision)")
            } else {
                Text("No sealed result yet.")
            }
        }
        .textSelection(.enabled)
    }

    private func validation(_ c: CandidateReview) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack {
                Text("Validation").font(.headline)
                badge(c.validation)
                Text("Write scope").font(.headline)
                badge(model.workspace.scope)
            }
            if !c.validationDetail.isEmpty { Text(c.validationDetail) }
            if !model.workspace.scopeDetail.isEmpty { Text("Scope: \(model.workspace.scopeDetail)") }
            ForEach(c.reasons, id: \.self) { Text("• \($0)") }
            if c.validation == .notRun {
                Text("Jojo has not validated this candidate. Tests the agent says it ran do not count.")
                    .foregroundStyle(.secondary)
            }
        }
        .textSelection(.enabled)
    }

    /// Four visually distinct states; colour is never the only signal.
    private func badge(_ v: ValidationVerdict) -> some View {
        let (symbol, color): (String, Color) = switch v {
        case .pass: ("checkmark.circle.fill", .green)
        case .fail: ("xmark.octagon.fill", .red)
        case .unknown: ("questionmark.diamond.fill", .orange)
        case .notRun: ("circle.dashed", .gray)
        }
        return Label(v.label, systemImage: symbol)
            .font(.system(.body, design: .monospaced).bold())
            .padding(.horizontal, 8).padding(.vertical, 2)
            .foregroundStyle(color)
            .overlay(RoundedRectangle(cornerRadius: 5).stroke(color))
    }

    private var files: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text("Changed files (\(model.workspace.changedFiles.count))").font(.headline)
            ForEach(model.workspace.changedFiles) { f in
                HStack {
                    Text(f.scope == "ALLOWED" ? "in scope" : f.scope == "BASELINE" ? "PROTECTED BASELINE" : f.scope == "OUT_OF_SCOPE" ? "OUT OF SCOPE" : "UNKNOWN")
                        .font(.caption.bold())
                        .foregroundStyle(f.scope == "ALLOWED" ? Color.secondary : Color.red)
                    Text("\(f.kind) \(f.path)").font(.system(.caption, design: .monospaced))
                }
            }
        }
        .textSelection(.enabled)
    }

    private func text(_ title: String, _ body: String) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(title).font(.headline)
            Text(body.isEmpty ? "(none)" : body)
                .font(.system(.caption, design: .monospaced))
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
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
