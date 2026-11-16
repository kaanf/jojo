@testable import JojoCore
import XCTest

/// Barrier-driven fake helper: nothing happens until the test delivers a frame or exit.
@MainActor
final class FakeHelper: HelperTransport {
    var onEvent: ((TransportEvent) -> Void)?
    var processIdentifier: Int32 { 4242 }
    var sent: [Envelope] = []
    var autoHello = true
    var epoch: Int64 = 7
    var compatible = true
    var failLaunch = false

    func start() throws {
        if failLaunch { throw JSONError("launch refused") }
    }

    func send(_ frame: [UInt8]) {
        let e = try! Envelope.parse(Array(frame.dropLast()))
        XCTAssertEqual(frame.last, 0x0A)
        sent.append(e)
        if e.type == "Hello", autoHello {
            reply(e, ["compatible": .bool(compatible), "epoch": .int(epoch)])
        }
    }

    func last(_ type: String) -> Envelope? { sent.last { $0.type == type } }
    func count(_ type: String) -> Int { sent.filter { $0.type == type }.count }
    var session: String { sent.first!.session }

    func reply(_ request: Envelope, _ payload: [String: JSON]) {
        deliver(Envelope(session: request.session, epoch: epoch, id: request.id, type: "Response", payload: payload))
    }

    func replyNull(_ request: Envelope) {
        var e = Envelope(session: request.session, epoch: epoch, id: request.id, type: "Response")
        e.payloadIsNull = true
        deliver(e)
    }

    func error(_ request: Envelope, _ code: String) {
        deliver(Envelope(session: request.session, epoch: epoch, id: request.id, type: "Error", payload: ["code": .text(code)]))
    }

    func ack(_ request: Envelope, _ response: String) {
        reply(request, ["acknowledged": .bool(true), "operation_completed": .bool(false), "command": .text(request.id),
                        "epoch": .int(epoch), "event_seq": .int(10), "response": .text(response), "replayed": .bool(false)])
    }

    func status(_ state: String, ticket: String?, active: Int64, receipt: JSON = .null) {
        let request = sent.last { $0.type == "CloseStatus" }!
        reply(request, ["state": .text(state), "ticket": ticket.map(JSON.text) ?? .null,
                        "active_count": .int(active), "receipt": receipt])
    }

    func emit(_ type: String, _ payload: [String: JSON], session: String? = nil, epoch: Int64? = nil) {
        deliver(Envelope(session: session ?? self.session, epoch: epoch ?? self.epoch, id: Envelope.zeroID,
                         type: type, payload: payload))
    }

    func deliver(_ e: Envelope) {
        onEvent?(.frame(Array(try! e.frame().dropLast())))
    }

    func end(_ status: Int32) { onEvent?(.ended(status: status)) }
    func breakChannel(_ reason: String) { onEvent?(.channelFailed(reason)) }
}

@MainActor
final class ManualScheduler: QuitScheduler {
    final class Pending: QuitTimer {
        var action: (@MainActor () -> Void)?
        let seconds: Double
        init(_ seconds: Double, _ action: @escaping @MainActor () -> Void) {
            self.seconds = seconds
            self.action = action
        }
        func cancel() { action = nil }
    }
    var pending: [Pending] = []

    func after(_ seconds: Double, _ action: @escaping @MainActor () -> Void) -> QuitTimer {
        let p = Pending(seconds, action)
        pending.append(p)
        return p
    }

    /// Fires every live timer once (a barrier step, not elapsed time).
    func fire() {
        let due = pending
        pending.removeAll()
        for p in due { p.action?() }
    }
    var live: Int { pending.filter { $0.action != nil }.count }
}

let ticketT = "000000000000000000000000000000a1"
let shutdownS = "800000000000000700000000000000b2"

func readyPayload(ticket: String = ticketT, sealed: Bool = true, stopped: Bool = true,
                  failures: [String] = [], outcome: String = "Ready") -> [String: JSON] {
    ["ticket": .text(ticket), "shutdown": .text(shutdownS), "event_seq": .int(42), "outcome": .text(outcome),
     "process_stop_confirmed": .bool(stopped), "persistence_sealed": .bool(sealed),
     "targets": .list([.text("attempt:1")]), "recovery_items": .list([.text("attempt:1")]),
     "failures": .list(failures.map(JSON.text))]
}
