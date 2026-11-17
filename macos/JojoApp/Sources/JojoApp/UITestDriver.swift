import AppKit
import JojoCore

/// Opt-in runtime GUI script (`--ui-script`). Steps run on the main run loop in common
/// modes and invoke the same entry points a user triggers: the Cmd-Q main-menu key
/// equivalent, a real quit Apple event, NSWindow.performClose and the Cancel/Confirm handlers.
/// It never signals a process. Unknown steps or a 30 s wait timeout stop the script.
@MainActor
final class UITestDriver {
    private var steps: [Substring]
    private unowned let delegate: AppDelegate
    private let trace: Tracer
    private var waitStarted: Date?
    /// P2-04: the attempt named by the last accepted Start, as the Start tab displays it.
    /// A later step may write `@` where a human would type that identity.
    private var attempt = ""

    init(script: String, delegate: AppDelegate, trace: Tracer) {
        steps = script.split(separator: ",")
        self.delegate = delegate
        self.trace = trace
    }

    func start() {
        schedule(0.2)
    }

    private func schedule(_ delay: Double) {
        let timer = Timer(timeInterval: delay, repeats: false) { [weak self] _ in
            MainActor.assumeIsolated { self?.step() }
        }
        RunLoop.main.add(timer, forMode: .common)
    }

    private func step() {
        guard let current = steps.first else {
            trace("script-done")
            return
        }
        let parts = current.split(separator: ":", maxSplits: 1).map(String.init)
        var argument = parts.count > 1 ? parts[1] : ""
        if parts[0] != "wait", parts[0] != "sleep", !attempt.isEmpty {
            argument = argument.replacingOccurrences(of: "@", with: attempt)
        }
        let model = delegate.model!
        if parts[0] == "wait" {
            if satisfied(argument, model) {
                waitStarted = nil
                trace("script wait:\(argument) ok")
            } else {
                let started = waitStarted ?? Date()
                waitStarted = started
                if Date().timeIntervalSince(started) > 30 {
                    trace("script-timeout wait:\(argument) phase=\(model.coordinator.phase.name)")
                    return
                }
                return schedule(0.05)
            }
        } else {
            trace("script \(current)")
            switch parts[0] {
            case "quit":
                // Cmd-Q through the real main menu key equivalent (SwiftUI Quit command).
                let key = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: .command, timestamp: 0,
                                           windowNumber: delegate.primary?.windowNumber ?? 0, context: nil,
                                           characters: "q", charactersIgnoringModifiers: "q", isARepeat: false, keyCode: 12)!
                trace("cmd-q handled=\(NSApp.mainMenu?.performKeyEquivalent(with: key) ?? false)")
            case "quit-ae":
                // A real kAEQuitApplication event sent to this process (Dock Quit/logout path).
                let target = NSAppleEventDescriptor(processIdentifier: ProcessInfo.processInfo.processIdentifier)
                let event = NSAppleEventDescriptor(eventClass: AEEventClass(kCoreEventClass), eventID: AEEventID(kAEQuitApplication),
                                                   targetDescriptor: target, returnID: AEReturnID(kAutoGenerateReturnID),
                                                   transactionID: AETransactionID(kAnyTransactionID))
                trace("quit-ae sent=\((try? event.sendEvent(options: [.noReply], timeout: 5)) != nil)")
            case "cancel": model.coordinator.cancel()
            case "confirm": model.coordinator.confirm()
            case "ack-unsettled": model.coordinator.acknowledgeUnsettled()
            case "disconnect": model.coordinator.disconnectAndQuit()
            case "stay": model.coordinator.stay()
            case "open-aux": delegate.showAuxiliary()
            case "close-aux": delegate.auxiliary?.performClose(nil)
            case "close-primary": delegate.primary?.performClose(nil)
            case "minimize-primary": delegate.primary?.miniaturize(nil)
            case "cancel-attempt": model.workspace.cancelAttempt(argument)
            case "review": model.workspace.loadReview(argument)
            case "decide":
                // The Review tab's decision buttons: sends exactly the bundle on screen.
                guard let action = DecisionAction(rawValue: argument) else {
                    trace("script-error decide \(argument)")
                    return
                }
                model.workspace.decide(action)
            case "review-state":
                let r = model.workspace.review
                trace("review-state subject=\(r?.subject ?? "none") revision=\(r?.revision ?? -1) hash=\(r?.hash ?? "") content=\(r?.contentHash ?? "") displayed=\(r?.displayedHash ?? "") context=\(r?.contextHash ?? "") prior=\(r?.priorDecision.map(String.init) ?? "none") status=\(model.workspace.status)")
            case "remember-attempt":
                // Reads the identity out of the status line a human would copy it from.
                let words = model.workspace.status.split(separator: " ").map(String.init)
                guard let at = words.firstIndex(of: "attempt"), at + 1 < words.count, Identity.valid(words[at + 1]) else {
                    trace("script-error remember-attempt status=\(model.workspace.status)")
                    return
                }
                attempt = words[at + 1]
                trace("attempt \(attempt)")
            case "recovery-state":
                // What the Recovery tab lists, through the same model it renders.
                let w = model.workspace
                trace("recovery-state admission=\(w.admission) digest=\(w.recoveryDigest) subjects=\(w.recovery.count) status=\(w.status)")
                for row in w.recovery {
                    trace("recovery-row attempt=\(row.attempt) assessment=\(row.assessment) findings=\(row.findings)")
                }
            case "candidate-result":
                // The sealed result as the Candidate tab shows it; `candidate-state` has the rest.
                let c = model.workspace.candidate
                trace("candidate-result attempt=\(c?.attempt ?? "none") sealed=\(c?.sealed ?? false) outcome=\(c?.outcome ?? "") reason=\(c?.reason ?? "") candidate=\(c?.candidate ?? "none") receipt=\(c?.receipt.map(String.init) ?? "none") validated=\(c?.validatedCandidate ?? "none")")
            case "candidate-text":
                // The diff and the validator report on screen, bounded for the trace.
                trace("candidate-diff \(String(model.workspace.diff.prefix(4096)))")
                trace("candidate-report \(String(model.workspace.validationReport.prefix(2048)))")
            case "recovery-inspect": model.workspace.inspectRecovery()
            case "recovery-start": model.workspace.startAfterRecovery()
            case "recovery-resolve":
                // attempt/choice, through the same entry point as the Recovery tab buttons.
                let fields = argument.split(separator: "/").map(String.init)
                guard fields.count == 2, let choice = RecoveryChoice(rawValue: fields[1]) else {
                    trace("script-error recovery-resolve \(argument)")
                    return
                }
                model.workspace.resolveRecovery(fields[0], choice)
            case "start":
                // prd/adr/plan/validation/step, through the Start tab's entry point.
                let f = argument.split(separator: "/").map(String.init)
                guard f.count == 5 else {
                    trace("script-error start \(argument)")
                    return
                }
                model.workspace.start(StartSubjects(prd: f[0], adr: f[1], plan: f[2], validation: f[3], step: f[4]))
            case "candidate": model.workspace.loadCandidate(argument)
            case "validate": model.workspace.validateCandidate()
            case "candidate-accept": model.workspace.decideCandidate(.Accept)
            case "candidate-reject": model.workspace.decideCandidate(.Reject)
            case "candidate-state":
                // What the Candidate tab shows, through the same model it renders.
                let w = model.workspace
                let c = w.candidate
                trace("candidate-state validation=\(c?.validation.label ?? "none") scope=\(w.scope.label) decision=\(c?.decision ?? "none") can-accept=\(c?.canAccept ?? false) files=\(w.changedFiles.map { "\($0.scope):\($0.path)" }.joined(separator: ";")) detail=\(c?.validationDetail ?? "") scope-detail=\(w.scopeDetail) reasons=\(c?.reasons.joined(separator: ";") ?? "") provider=\(c?.providerTerminal ?? "") log=\(w.agentLog.joined(separator: ";")) diff-bytes=\(w.diff.utf8.count) report-bytes=\(w.validationReport.utf8.count) pending=\(w.commandPending) status=\(w.status)")
            case "state":
                trace("state phase=\(model.coordinator.phase.name) connection=\(model.client.connection) duplicates=\(model.coordinator.duplicateRequests) primary-visible=\(delegate.primary?.isVisible ?? false) aux=\(delegate.auxiliary != nil) review-status=\(model.workspace.status) recovery=\(model.workspace.recovery.count) admission=\(model.workspace.admission) pending=\(model.workspace.commandPending)")
            case "repairs":
                // M4: Agent Runs shown, so its repairs are read.
                model.workspace.repairsShown = true
                model.workspace.loadRepairs()
                model.workspace.loadAttempts()
            case "repairs-state":
                // What Agent Runs lists, through the same state it renders.
                let w = model.workspace
                let state = AgentRunsState(connection: model.client.connection, admission: w.admission,
                                           attempts: AgentRunsState.attempts(in: w.rows), recovery: w.recovery,
                                           result: nil, log: [], commandPending: w.commandPending, repairs: w.repairs)
                let rows = state.runs.map { "\($0.short)|\($0.pill.label)|\($0.line)|\($0.verdict?.text ?? "-")|\($0.repair?.hold ?? "-")" }
                trace("repairs-state load=\(w.repairsLoad) rows=\(rows.joined(separator: " || "))")
            case "sleep": break
            default:
                trace("script-error unknown step \(current)")
                return
            }
        }
        steps.removeFirst()
        schedule(parts[0] == "sleep" ? (Double(argument) ?? 0) / 1000 : 0.05)
    }

    private func satisfied(_ condition: String, _ model: AppModel) -> Bool {
        switch condition {
        case "ready":
            if case .ready = model.client.connection { return true }
            return false
        case "ended":
            if case .ended = model.client.connection { return true }
            return false
        case "lost":
            if case .channelLost = model.client.connection { return true }
            return false
        case "repairs":
            return model.workspace.repairsLoad == .loaded
        case "review-status":
            return !model.workspace.status.isEmpty
        case _ where condition.hasPrefix("status="):
            return model.workspace.status.contains(condition.dropFirst("status=".count))
        case _ where condition.hasPrefix("validation="):
            return model.workspace.candidate?.validation.label == String(condition.dropFirst("validation=".count))
        case _ where condition.hasPrefix("scope="):
            return model.workspace.scope.label == String(condition.dropFirst("scope=".count))
        case "sealed":
            return model.workspace.candidate?.sealed == true
        case _ where condition.hasPrefix("outcome="):
            return model.workspace.candidate?.outcome == String(condition.dropFirst("outcome=".count))
        case _ where condition.hasPrefix("decision="):
            return model.workspace.candidate?.decision == String(condition.dropFirst("decision=".count))
        case _ where condition.hasPrefix("files="):
            return model.workspace.changedFiles.count == Int(condition.dropFirst("files=".count))
        case _ where condition.hasPrefix("log="):
            return model.workspace.agentLog.contains { $0.contains(condition.dropFirst("log=".count)) }
        case "diff":
            return !model.workspace.diff.isEmpty
        case "report":
            return !model.workspace.validationReport.isEmpty
        case _ where condition.hasPrefix("recovery="):
            return model.workspace.recovery.count == Int(condition.dropFirst("recovery=".count))
        case _ where condition.hasPrefix("choices="):
            // The choices the helper offers for the first listed subject, e.g. `choices=[Hold]`.
            return model.workspace.recovery.first?.structured?.choicesText == String(condition.dropFirst("choices=".count))
        case _ where condition.hasPrefix("admission="):
            return model.workspace.admission == condition.dropFirst("admission=".count)
        default:
            return model.coordinator.phase.name == condition
        }
    }
}
