import Foundation

public enum HelperConnection: Equatable, Sendable {
    case notStarted
    case launchFailed(String)
    case handshaking
    case ready(epoch: Int64)
    /// Protocol mismatch: diagnostics only, no mutation (P1-13).
    case incompatible(epoch: Int64)
    /// The pipe is unusable; the helper may still be alive. State is unknown, never stopped.
    case channelLost(String)
    /// Helper exited (stdout drained). Exit alone is never shutdown proof.
    case ended(status: Int32)

    public var helperMayBeAlive: Bool {
        switch self {
        case .handshaking, .ready, .incompatible, .channelLost: return true
        case .notStarted, .launchFailed, .ended: return false
        }
    }
}

/// Acknowledgement is not completion: a `.response` to a command proves only its commit.
public enum Reply: Equatable, Sendable {
    case response([String: JSON])
    /// Response with a null payload: the queried record does not exist.
    case null
    /// Response with a list payload (Events).
    case list([JSON])
    case error(String)
    /// No answer will arrive (channel lost/helper ended). The outcome is unknown.
    case unanswered(String)
}

/// Request/response correlation over one helper session. Mutating commands are tracked
/// until answered; unanswered ones stay outstanding so quitting must reconcile them by
/// receipt lookup under the same identity, never by resending with a new identity.
@MainActor
public final class HelperClient {
    public let session: String
    public private(set) var connection: HelperConnection = .notStarted {
        didSet { if connection != oldValue { onConnectionChange?(connection) } }
    }
    public private(set) var epoch: Int64 = 0
    public private(set) var outstandingCommands: [String: String] = [:]
    public var onConnectionChange: ((HelperConnection) -> Void)?
    /// Helper-initiated frames (ShutdownReady/ShutdownFailure). Validation is the caller's.
    public var onUnsolicited: ((Envelope) -> Void)?
    public var onOutstandingChange: (() -> Void)?
    public var trace: ((String) -> Void)?

    private let transport: HelperTransport
    private let newID: () -> String
    private var pending: [String: (Reply) -> Void] = [:]

    public init(transport: HelperTransport, session: String = Identity.randomClientID(),
                newID: @escaping () -> String = Identity.randomClientID) {
        self.transport = transport
        self.session = session
        self.newID = newID
        transport.onEvent = { [weak self] in self?.handle($0) }
    }

    public var processIdentifier: Int32 { transport.processIdentifier }

    public func start() {
        guard connection == .notStarted else { return }
        do {
            try transport.start()
        } catch {
            connection = .launchFailed(String(describing: error))
            return
        }
        connection = .handshaking
        let id = newID()
        pending[id] = { [weak self] reply in
            guard let self else { return }
            guard case .response(let p) = reply, let epoch = p["epoch"]?.integer, let compatible = p["compatible"]?.boolean else {
                if self.connection == .handshaking { self.connection = .channelLost("handshake failed: \(reply)") }
                return
            }
            self.epoch = epoch
            self.connection = compatible ? .ready(epoch: epoch) : .incompatible(epoch: epoch)
        }
        write(Envelope(session: session, epoch: 0, id: id, type: "Hello"))
    }

    public func query(_ type: String, _ payload: [String: JSON] = [:], _ completion: @escaping (Reply) -> Void) {
        guard case .ready = connection else {
            if case .incompatible = connection { return send(type, payload, nil, completion) }
            return completion(.unanswered("helper unavailable"))
        }
        send(type, payload, nil, completion)
    }

    /// Returns the command identity, or nil when no mutation may be sent.
    @discardableResult
    public func command(_ type: String, _ payload: [String: JSON], revision: Int64? = nil,
                        _ completion: @escaping (Reply) -> Void) -> String? {
        guard case .ready = connection else {
            completion(.unanswered("mutations unavailable: \(connection)"))
            return nil
        }
        let id = newID()
        outstandingCommands[id] = type
        onOutstandingChange?()
        send(type, payload, revision, id: id) { [weak self] reply in
            if case .unanswered = reply {} else {
                self?.outstandingCommands[id] = nil
                self?.onOutstandingChange?()
            }
            completion(reply)
        }
        return id
    }

    /// The user explicitly abandons uncertain commands; they are never resent.
    public func abandonOutstanding() -> [String] {
        let ids = outstandingCommands.keys.sorted()
        outstandingCommands.removeAll()
        onOutstandingChange?()
        return ids
    }

    /// Reconciled by durable receipt lookup under the original identity.
    public func settleOutstanding(_ id: String) {
        outstandingCommands[id] = nil
        onOutstandingChange?()
    }

    private func send(_ type: String, _ payload: [String: JSON], _ revision: Int64?, id: String? = nil,
                      _ completion: @escaping (Reply) -> Void) {
        let id = id ?? newID()
        pending[id] = completion
        write(Envelope(session: session, epoch: epoch, id: id, revision: revision, type: type, payload: payload))
    }

    private func write(_ e: Envelope) {
        guard let frame = try? e.frame() else {
            pending.removeValue(forKey: e.id)?(.error("FrameTooLarge"))
            return
        }
        trace?("send \(e.type) \(e.id)")
        transport.send(frame)
    }

    func handle(_ event: TransportEvent) {
        switch event {
        case .frame(let bytes):
            let e: Envelope
            do {
                e = try Envelope.parse(bytes)
            } catch {
                return lose("malformed helper frame: \(error)")
            }
            guard e.major == Envelope.major else { return lose("helper major version \(e.major)") }
            if e.id == Envelope.zeroID {
                if e.type == "Error" {
                    return lose("helper protocol error: \(e.payload["code"]?.string ?? "?")")
                }
                onUnsolicited?(e)
                return
            }
            guard e.session == session, let completion = pending.removeValue(forKey: e.id) else {
                trace?("ignored unmatched frame \(e.type) \(e.id)")
                return
            }
            if connection != .handshaking, e.epoch != epoch {
                return lose("helper epoch changed")
            }
            switch e.type {
            case "Response": completion(e.payloadIsNull ? .null : e.payloadList.map(Reply.list) ?? .response(e.payload))
            case "Error": completion(.error(e.payload["code"]?.string ?? "Error"))
            default: lose("unexpected response type \(e.type)")
            }
        case .channelFailed(let reason):
            lose(reason)
        case .ended(let status):
            connection = .ended(status: status)
            drain("helper ended with status \(status)")
        }
    }

    private func lose(_ reason: String) {
        if case .ended = connection { return }
        if case .channelLost = connection {} else { connection = .channelLost(reason) }
        drain(reason)
    }

    private func drain(_ reason: String) {
        let waiting = pending
        pending.removeAll()
        for (_, completion) in waiting.sorted(by: { $0.key < $1.key }) { completion(.unanswered(reason)) }
    }
}
