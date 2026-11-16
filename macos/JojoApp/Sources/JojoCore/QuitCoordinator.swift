import Foundation

public enum QuitSource: String, Sendable {
    case quitCommand
    case lastPrimaryWindow
}

/// Maps onto NSApplication.TerminateReply without importing AppKit.
public enum TerminationDecision: Equatable, Sendable {
    case now
    case later
    case cancel
}

/// A validated, session/epoch/ticket-bound READY receipt. Not yet clean: the helper's
/// successful exit must also be observed (P1-13 ClientClose rule).
public struct ShutdownProof: Equatable, Sendable {
    public let ticket: String
    public let shutdown: String
    public let eventSeq: Int64
    public let targets: [String]
    public let recoveryItems: [String]
}

public enum QuitFailure: Equatable, Sendable {
    /// Pending commands whose commit could not be confirmed by receipt lookup.
    case unsettledCommands([String])
    case closeRejected(String)
    case shutdownFailed(String)
    case invalidProof(String)
    case helperEnded(status: Int32, afterProof: Bool)
    case exitTimeout
    case channelLost(String)
    case unexpectedState(String)

    /// Retrying PrepareClose is safe; the helper shares an existing close operation.
    var retryable: Bool {
        switch self {
        case .closeRejected, .unexpectedState: return true
        default: return false
        }
    }
}

public enum QuitPhase: Equatable, Sendable {
    case idle
    case reconciling(remaining: Int)
    case preparing
    case confirmRequired(ticket: String, activeCount: Int)
    case confirming(ticket: String)
    case cancelling(ticket: String)
    /// No active work: same finalization checks, no confirmation (SHUTDOWN_RECOVERY).
    case finalizing
    case stopping(ticket: String, activeCount: Int?)
    case awaitingExit(ShutdownProof)
    case clean(ShutdownProof)
    case failed(QuitFailure)
    /// The helper cannot complete a verified close; quitting means EOF emergency handling.
    case disconnectRequired(String)

    public var name: String {
        switch self {
        case .idle: return "idle"
        case .reconciling: return "reconciling"
        case .preparing: return "preparing"
        case .confirmRequired: return "confirmRequired"
        case .confirming: return "confirming"
        case .cancelling: return "cancelling"
        case .finalizing: return "finalizing"
        case .stopping: return "stopping"
        case .awaitingExit: return "awaitingExit"
        case .clean: return "clean"
        case .failed: return "failed"
        case .disconnectRequired: return "disconnectRequired"
        }
    }

    public var inProgress: Bool {
        switch self {
        case .reconciling, .preparing, .confirmRequired, .confirming, .cancelling, .finalizing,
             .stopping, .awaitingExit, .disconnectRequired:
            return true
        case .idle, .clean, .failed:
            return false
        }
    }
}

/// Why the GUI was allowed to exit. Only `.clean` represents a verified shutdown.
public enum QuitOutcome: Equatable, Sendable {
    case clean(ShutdownProof, unconfirmedCommands: [String])
    case noHelper(String)
    case helperGoneUnverified(status: Int32)
    case emergencyDisconnect(String)

    public var label: String {
        switch self {
        case .clean(_, let unconfirmed): return unconfirmed.isEmpty ? "clean" : "clean-with-unconfirmed-commands"
        case .noHelper: return "no-helper"
        case .helperGoneUnverified: return "unverified-helper-gone"
        case .emergencyDisconnect: return "emergency-disconnect-unclean"
        }
    }
}

@MainActor
public protocol QuitTimer: AnyObject {
    func cancel()
}

@MainActor
public protocol QuitScheduler: AnyObject {
    func after(_ seconds: Double, _ action: @escaping @MainActor () -> Void) -> QuitTimer
}

/// One coordinator for Cmd-Q, menu Quit and last-primary-window close. It never signals a
/// process: stopping is requested from the helper (P1-12) and proven by its READY receipt
/// plus the helper's observed successful exit. Elapsed time never confirms or cleans.
@MainActor
public final class QuitCoordinator {
    public private(set) var phase: QuitPhase = .idle {
        didSet {
            if phase != oldValue {
                trace?("phase \(phase.name) \(phase)")
                onChange?()
            }
        }
    }
    public private(set) var outcome: QuitOutcome?
    public private(set) var duplicateRequests = 0
    public private(set) var abandonedCommands: [String] = []
    public private(set) var confirmedAt: Date?
    /// NSApplication.reply(toApplicationShouldTerminate:), called at most once per deferral.
    public var reply: ((Bool) -> Void)?
    /// Asks AppKit to start termination (used after an explicit disconnect choice).
    public var requestTermination: (() -> Void)?
    public var onChange: (() -> Void)?
    public var onAttention: (() -> Void)?
    public var trace: ((String) -> Void)?

    public var reconcileWait = 3.0
    public var statusPoll = 0.5
    public var exitWait = 10.0

    private let client: HelperClient
    private let scheduler: QuitScheduler
    private var replyPending = false
    private var ticket: String?
    private var readyWithoutTicket = false
    private var awaitingHandshake = false
    private var statusInFlight = false
    private var disconnectChosen = false
    private var timers: [QuitTimer] = []

    public init(client: HelperClient, scheduler: QuitScheduler) {
        self.client = client
        self.scheduler = scheduler
        client.onUnsolicited = { [weak self] in self?.unsolicited($0) }
        client.onOutstandingChange = { [weak self] in self?.outstandingChanged() }
    }

    /// The single entry point for every quit path. Duplicates share the running operation.
    public func requestQuit(_ source: QuitSource) -> TerminationDecision {
        trace?("quit-request \(source.rawValue) in \(phase.name)")
        if disconnectChosen {
            return .now
        }
        if phase.inProgress {
            duplicateRequests += 1
            trace?("duplicate-quit \(duplicateRequests)")
            onAttention?()
            return .cancel
        }
        switch phase {
        case .clean:
            return .now
        case .failed(let failure) where !failure.retryable:
            if case .ended(let status) = client.connection {
                outcome = .helperGoneUnverified(status: status)
                return .now
            }
            // SHUTDOWN_FAILED: stay in Jojo, keep the failure visible (no clean claim).
            onAttention?()
            return .cancel
        default:
            break
        }
        switch client.connection {
        case .notStarted, .launchFailed:
            outcome = .noHelper("\(client.connection)")
            return .now
        case .ended(let status):
            outcome = .helperGoneUnverified(status: status)
            return .now
        case .channelLost(let reason):
            replyPending = true
            phase = .disconnectRequired("helper channel lost: \(reason)")
            return .later
        case .incompatible:
            replyPending = true
            phase = .disconnectRequired("protocol mismatch: the helper accepts diagnostics only")
            return .later
        case .handshaking:
            replyPending = true
            awaitingHandshake = true
            phase = .preparing
            return .later
        case .ready:
            replyPending = true
            reconcile()
            return .later
        }
    }

    /// Cancel: sends CancelClose only. Running agents are untouched; no stop was requested.
    public func cancel() {
        guard case .confirmRequired(let t, _) = phase else {
            trace?("cancel ignored in \(phase.name)")
            return
        }
        phase = .cancelling(ticket: t)
        finishReply(false)
        client.command(.CancelClose, ["ticket": .text(t)]) { [weak self] reply in
            guard let self, case .cancelling = self.phase else { return }
            switch reply {
            case .response:
                self.ticket = nil
                self.phase = .idle
            case .error(let code):
                self.fail(.closeRejected("CancelClose: \(code)"))
            case .null, .list, .unanswered:
                break
            }
        }
    }

    /// Confirm Quit: requests coordinated shutdown of all active work, then waits for proof.
    public func confirm() {
        guard case .confirmRequired(let t, _) = phase else {
            trace?("confirm ignored in \(phase.name)")
            return
        }
        phase = .confirming(ticket: t)
        client.command(.ConfirmClose, ["ticket": .text(t)]) { [weak self] reply in
            guard let self, case .confirming = self.phase else { return }
            switch reply {
            case .response:
                self.confirmedAt = Date()
                self.phase = .stopping(ticket: t, activeCount: nil)
                self.pollStatus()
            case .error(let code):
                self.fail(.closeRejected("ConfirmClose: \(code)"))
            case .null, .list, .unanswered:
                break
            }
        }
    }

    /// The user explicitly gives up on unconfirmed commands. They are never resent.
    public func acknowledgeUnsettled() {
        guard case .failed(.unsettledCommands) = phase else { return }
        abandonedCommands += client.abandonOutstanding()
        phase = .idle
    }

    /// Quit without a verified close: the GUI exits, the helper sees EOF and runs its
    /// existing emergency stop. Recorded as unclean, never as clean.
    public func disconnectAndQuit() {
        let reason: String
        switch phase {
        case .disconnectRequired(let r): reason = r
        case .failed(let f) where !f.retryable: reason = "after failure: \(f)"
        default:
            trace?("disconnect ignored in \(phase.name)")
            return
        }
        disconnectChosen = true
        outcome = .emergencyDisconnect(reason)
        trace?("disconnect-chosen \(reason)")
        if replyPending {
            finishReply(true)
        } else {
            requestTermination?()
        }
    }

    /// Stay in Jojo from the disconnect warning.
    public func stay() {
        guard case .disconnectRequired = phase else { return }
        phase = .idle
        finishReply(false)
    }

    /// Forwarded from the client's connection observer.
    public func connectionChanged(_ connection: HelperConnection) {
        switch connection {
        case .ready:
            if awaitingHandshake {
                awaitingHandshake = false
                reconcile()
            }
        case .incompatible:
            if awaitingHandshake {
                awaitingHandshake = false
                phase = .disconnectRequired("protocol mismatch: the helper accepts diagnostics only")
            }
        case .channelLost(let reason):
            awaitingHandshake = false
            switch phase {
            case .preparing, .reconciling, .confirmRequired, .confirming, .finalizing, .stopping, .awaitingExit, .cancelling:
                fail(.channelLost(reason))
            default:
                break
            }
        case .ended(let status):
            awaitingHandshake = false
            ended(status)
        case .launchFailed(let reason):
            if case .preparing = phase { fail(.channelLost(reason)) }
        case .notStarted, .handshaking:
            break
        }
    }

    // MARK: - Flow

    private func reconcile() {
        let ids = client.outstandingCommands.keys.sorted()
        if ids.isEmpty { return prepare() }
        phase = .reconciling(remaining: ids.count)
        timers.append(scheduler.after(reconcileWait) { [weak self] in self?.lookupReceipts() })
    }

    private func outstandingChanged() {
        guard case .reconciling = phase else { return }
        let n = client.outstandingCommands.count
        if n == 0 { prepare() } else { phase = .reconciling(remaining: n) }
    }

    /// Receipt lookup under the original identity; a missing receipt is unsettled, not retried.
    private func lookupReceipts() {
        guard case .reconciling = phase else { return }
        var remaining = Set(client.outstandingCommands.keys)
        var unsettled: [String] = []
        for id in remaining.sorted() {
            client.query(.Receipt, ["command": .text(id)]) { [weak self] reply in
                guard let self, case .reconciling = self.phase else { return }
                remaining.remove(id)
                if case .response(let p) = reply, p["acknowledged"] == .bool(true), p["command"] == .text(id) {
                    self.client.settleOutstanding(id)
                } else if self.client.outstandingCommands[id] != nil {
                    unsettled.append(id)
                }
                guard remaining.isEmpty, case .reconciling = self.phase else { return }
                if unsettled.isEmpty, self.client.outstandingCommands.isEmpty {
                    self.prepare()
                } else {
                    self.fail(.unsettledCommands(unsettled.sorted()))
                }
            }
        }
    }

    private func prepare() {
        cancelTimers()
        ticket = nil
        readyWithoutTicket = false
        phase = .preparing
        let sent = client.command(.PrepareClose) { [weak self] reply in
            guard let self, case .preparing = self.phase else { return }
            switch reply {
            case .response(let p):
                let response = p["response"]?.string ?? ""
                if response == "NO_ACTIVE" {
                    // The helper finalizes immediately and exits; the READY receipt is the
                    // only ticket carrier, bound by this session and epoch.
                    self.readyWithoutTicket = true
                    self.phase = .finalizing
                } else if response.hasPrefix("CONFIRM_REQUIRED") || response == "EXISTING" {
                    self.pollStatus()
                } else {
                    self.fail(.unexpectedState("PrepareClose: \(response)"))
                }
            case .error(let code):
                self.fail(.closeRejected("PrepareClose: \(code)"))
            case .null, .list:
                self.fail(.unexpectedState("PrepareClose: malformed receipt"))
            case .unanswered:
                break
            }
        }
        if sent == nil { fail(.channelLost("PrepareClose could not be sent")) }
    }

    private func pollStatus() {
        guard !statusInFlight else { return }
        statusInFlight = true
        client.query(.CloseStatus) { [weak self] reply in
            guard let self else { return }
            self.statusInFlight = false
            if case .response(let p) = reply { self.applyStatus(p) }
            switch self.phase {
            case .preparing, .confirmRequired, .stopping:
                self.timers.append(self.scheduler.after(self.statusPoll) { [weak self] in self?.pollStatus() })
            default:
                break
            }
        }
    }

    private func applyStatus(_ p: [String: JSON]) {
        let state = p["state"]?.string ?? ""
        let t = p["ticket"]?.string
        let active = Int(p["active_count"]?.integer ?? 0)
        switch phase {
        case .preparing:
            switch state {
            case "AwaitingConfirmation":
                guard let t, Identity.valid(t) else { return fail(.unexpectedState("ticketless confirmation")) }
                ticket = t
                phase = .confirmRequired(ticket: t, activeCount: active)
            case "Stopping", "Finalizing", "Ready":
                guard let t, Identity.valid(t) else { return fail(.unexpectedState("ticketless close")) }
                ticket = t
                phase = .stopping(ticket: t, activeCount: active)
            case "Failed":
                fail(.shutdownFailed(summary(p["receipt"])))
            default:
                fail(.unexpectedState("CloseStatus \(state)"))
            }
        case .confirmRequired(let current, _):
            // A changing count is displayed; reaching zero is never the user's confirmation.
            if state == "AwaitingConfirmation", t == current {
                phase = .confirmRequired(ticket: current, activeCount: active)
            } else {
                fail(.unexpectedState("ticket changed while awaiting confirmation: \(state)"))
            }
        case .stopping(let current, _):
            if state == "Failed" {
                fail(.shutdownFailed(summary(p["receipt"])))
            } else if state == "Stopping" || state == "Finalizing" {
                phase = .stopping(ticket: current, activeCount: active)
            }
        default:
            break
        }
    }

    private func unsolicited(_ e: Envelope) {
        guard e.session == client.session, e.epoch == client.epoch else {
            trace?("ignored foreign \(e.type)")
            return
        }
        switch e.type {
        case "ShutdownReady":
            switch phase {
            case .preparing, .finalizing, .confirming, .stopping:
                break
            default:
                trace?("ShutdownReady ignored in \(phase.name)")
                return
            }
            switch validate(e.payload) {
            case .success(let proof):
                phase = .awaitingExit(proof)
                timers.append(scheduler.after(exitWait) { [weak self] in
                    guard let self, case .awaitingExit = self.phase else { return }
                    self.fail(.exitTimeout)
                })
            case .failure(let error):
                fail(.invalidProof(error.description))
            }
        case "ShutdownFailure":
            fail(.shutdownFailed(summary(.object(e.payload))))
        default:
            trace?("unsolicited \(e.type) ignored")
        }
    }

    /// Mirrors jojo_domain::ipc::ClientClose: matching ticket, nonzero shutdown and event,
    /// outcome Ready, stop confirmed, persistence sealed and no failures.
    private func validate(_ p: [String: JSON]) -> Result<ShutdownProof, JSONError> {
        guard let t = p["ticket"]?.string, Identity.valid(t) else { return .failure(JSONError("ticket")) }
        if let expected = ticket {
            guard t == expected else { return .failure(JSONError("ticket mismatch")) }
        } else if !readyWithoutTicket {
            return .failure(JSONError("READY without a bound close ticket"))
        }
        guard let s = p["shutdown"]?.string, Identity.valid(s), s != Envelope.zeroID,
              let seq = p["event_seq"]?.integer, seq > 0,
              p["outcome"] == .text("Ready"),
              p["process_stop_confirmed"] == .bool(true),
              p["persistence_sealed"] == .bool(true),
              p["failures"] == .list([]) else {
            return .failure(JSONError("incomplete shutdown proof"))
        }
        let strings = { (key: String) in (p[key]?.items ?? []).compactMap(\.string) }
        return .success(ShutdownProof(ticket: t, shutdown: s, eventSeq: seq,
                                      targets: strings("targets"), recoveryItems: strings("recovery_items")))
    }

    private func ended(_ status: Int32) {
        switch phase {
        case .awaitingExit(let proof):
            if status == 0 {
                cancelTimers()
                outcome = .clean(proof, unconfirmedCommands: abandonedCommands)
                phase = .clean(proof)
                finishReply(true)
            } else {
                fail(.helperEnded(status: status, afterProof: true))
            }
        case .reconciling, .preparing, .confirmRequired, .confirming, .cancelling, .finalizing, .stopping:
            fail(.helperEnded(status: status, afterProof: false))
        default:
            break
        }
    }

    private func fail(_ failure: QuitFailure) {
        cancelTimers()
        phase = .failed(failure)
        finishReply(false)
    }

    private func finishReply(_ terminate: Bool) {
        guard replyPending else { return }
        replyPending = false
        trace?("reply \(terminate)")
        reply?(terminate)
    }

    private func cancelTimers() {
        timers.forEach { $0.cancel() }
        timers.removeAll()
    }

    private func summary(_ value: JSON?) -> String {
        guard let value, case .object(let p) = value else { return "no receipt" }
        if let code = p["code"]?.string { return code }
        let failures = (p["failures"]?.items ?? []).compactMap(\.string)
        return "outcome=\(p["outcome"]?.string ?? "?") stop_confirmed=\(p["process_stop_confirmed"]?.boolean ?? false) sealed=\(p["persistence_sealed"]?.boolean ?? false) failures=\(failures)"
    }
}
