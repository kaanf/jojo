import Foundation

/// P1-13 envelope: exactly eight fields, ≤ 65,536 bytes excluding LF. Protocol 1.1 (P2-02)
/// adds commands only; the frame is unchanged.
public struct Envelope: Equatable, Sendable {
    public static let major: Int64 = 1
    public static let minor: Int64 = 1
    public static let maxFrame = 64 * 1024
    public static let zeroID = String(repeating: "0", count: 32)

    public var major: Int64
    public var minor: Int64
    public var session: String
    public var epoch: Int64
    public var id: String
    public var revision: Int64?
    public var type: String
    public var payload: [String: JSON]
    /// Observed P1-13 helper behavior beyond the documented object payload: an absent
    /// receipt/bundle is `"payload":null` and Events is a list. Accepted only on `Response`
    /// frames and surfaced as `Reply.null` / `Reply.list`, never as an empty object.
    public var payloadIsNull = false
    public var payloadList: [JSON]?

    public init(session: String, epoch: Int64, id: String, revision: Int64? = nil, type: String,
                payload: [String: JSON] = [:], major: Int64 = Envelope.major, minor: Int64 = Envelope.minor) {
        self.major = major
        self.minor = minor
        self.session = session
        self.epoch = epoch
        self.id = id
        self.revision = revision
        self.type = type
        self.payload = payload
    }

    public static func parse(_ line: [UInt8]) throws -> Envelope {
        guard line.count <= maxFrame else { throw JSONError("frame too large") }
        guard case .object(let f) = try StrictJSON.parse(line), f.count == 8 else {
            throw JSONError("envelope requires exactly eight fields")
        }
        guard let major = f["major"]?.integer, major >= 0,
              let minor = f["minor"]?.integer, minor >= 0,
              let session = f["session"]?.string, Identity.valid(session),
              let epoch = f["epoch"]?.integer, epoch >= 0,
              let id = f["id"]?.string, Identity.valid(id),
              let revisionValue = f["revision"],
              let type = f["type"]?.string, type.utf8.count <= 64,
              let payloadValue = f["payload"] else {
            throw JSONError("invalid envelope fields")
        }
        var payload: [String: JSON] = [:]
        if case .object(let fields) = payloadValue {
            payload = fields
        } else if type != "Response" || !(payloadValue.isNull || payloadValue.items != nil) {
            throw JSONError("payload must be an object")
        }
        var revision: Int64?
        if !revisionValue.isNull {
            guard let r = revisionValue.integer, r >= 0 else { throw JSONError("invalid revision") }
            revision = r
        }
        var e = Envelope(session: session, epoch: epoch, id: id, revision: revision, type: type,
                         payload: payload, major: major, minor: minor)
        e.payloadIsNull = payloadValue.isNull
        e.payloadList = payloadValue.items
        return e
    }

    public var json: JSON {
        .object([
            "major": .int(major), "minor": .int(minor), "session": .text(session),
            "epoch": .int(epoch), "id": .text(id), "revision": revision.map(JSON.int) ?? .null,
            "type": .text(type), "payload": payloadIsNull ? .null : payloadList.map(JSON.list) ?? .object(payload),
        ])
    }

    public func frame() throws -> [UInt8] {
        let bytes = Array(StrictJSON.encode(json).utf8)
        guard bytes.count <= Envelope.maxFrame else { throw JSONError("frame too large") }
        return bytes + [0x0A]
    }
}

public enum Identity {
    public static func valid(_ s: String) -> Bool {
        s.utf8.count == 32 && s.utf8.allSatisfy { (0x30...0x39).contains($0) || (0x61...0x66).contains($0) }
    }
    /// Client command identities: random 127-bit values in the low half of the ID space.
    /// Receipts are durable across epochs, so a per-session counter would replay old receipts.
    public static func randomClientID() -> String {
        var bytes = withUnsafeBytes(of: UUID().uuid) { Array($0) }
        bytes[0] &= 0x7F
        let id = bytes.map { String(format: "%02x", $0) }.joined()
        return id == Envelope.zeroID ? randomClientID() : id
    }
}

/// Splits fragmented helper stdout into frames. Oversize or truncated input is fatal.
public struct FrameSplitter {
    private var pending: [UInt8] = []
    public init() {}

    public mutating func push(_ data: some Sequence<UInt8>) throws -> [[UInt8]] {
        var frames: [[UInt8]] = []
        for byte in data {
            if byte == 0x0A {
                frames.append(pending)
                pending.removeAll(keepingCapacity: true)
            } else {
                guard pending.count < Envelope.maxFrame else { throw JSONError("frame too large") }
                pending.append(byte)
            }
        }
        return frames
    }

    public func finish() throws {
        guard pending.isEmpty else { throw JSONError("truncated frame") }
    }
}
