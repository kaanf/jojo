import Foundation

/// Strict JSON matching the helper's P1-13 parser: integers only, no duplicate keys,
/// bounded depth, valid Unicode. Anything else fails closed.
public indirect enum JSON: Equatable, Sendable {
    case null
    case bool(Bool)
    case int(Int64)
    case text(String)
    case list([JSON])
    case object([String: JSON])

    public subscript(key: String) -> JSON? {
        if case .object(let fields) = self { return fields[key] }
        return nil
    }
    public var string: String? { if case .text(let s) = self { return s }; return nil }
    public var integer: Int64? { if case .int(let n) = self { return n }; return nil }
    public var boolean: Bool? { if case .bool(let b) = self { return b }; return nil }
    public var items: [JSON]? { if case .list(let v) = self { return v }; return nil }
    public var isNull: Bool { self == .null }
}

public struct JSONError: Error, Equatable, CustomStringConvertible {
    public let description: String
    public init(_ description: String) { self.description = description }
}

public enum StrictJSON {
    static let maxDepth = 32

    public static func parse(_ bytes: [UInt8]) throws -> JSON {
        var parser = Parser(bytes: bytes)
        let value = try parser.value(depth: 0)
        parser.skipSpace()
        guard parser.index == bytes.count else { throw JSONError("trailing data") }
        return value
    }

    /// Canonical encoding with sorted keys, like the helper's encoder.
    public static func encode(_ value: JSON) -> String {
        var out = ""
        write(value, &out)
        return out
    }

    private static func write(_ value: JSON, _ out: inout String) {
        switch value {
        case .null: out += "null"
        case .bool(let b): out += b ? "true" : "false"
        case .int(let n): out += String(n)
        case .text(let s): writeString(s, &out)
        case .list(let items):
            out += "["
            for (i, item) in items.enumerated() {
                if i > 0 { out += "," }
                write(item, &out)
            }
            out += "]"
        case .object(let fields):
            out += "{"
            for (i, key) in fields.keys.sorted().enumerated() {
                if i > 0 { out += "," }
                writeString(key, &out)
                out += ":"
                write(fields[key]!, &out)
            }
            out += "}"
        }
    }

    private static func writeString(_ s: String, _ out: inout String) {
        out += "\""
        for scalar in s.unicodeScalars {
            switch scalar {
            case "\"": out += "\\\""
            case "\\": out += "\\\\"
            case "\n": out += "\\n"
            case "\r": out += "\\r"
            case "\t": out += "\\t"
            default:
                if scalar.value < 0x20 {
                    out += String(format: "\\u%04x", scalar.value)
                } else {
                    out.unicodeScalars.append(scalar)
                }
            }
        }
        out += "\""
    }

    private struct Parser {
        let bytes: [UInt8]
        var index = 0

        mutating func skipSpace() {
            while index < bytes.count, [0x20, 0x09, 0x0A, 0x0D].contains(bytes[index]) { index += 1 }
        }

        mutating func literal(_ word: String) throws {
            let w = Array(word.utf8)
            guard index + w.count <= bytes.count, Array(bytes[index..<index + w.count]) == w else {
                throw JSONError("invalid literal")
            }
            index += w.count
        }

        mutating func value(depth: Int) throws -> JSON {
            guard depth <= StrictJSON.maxDepth else { throw JSONError("nesting too deep") }
            skipSpace()
            guard index < bytes.count else { throw JSONError("unexpected end") }
            switch bytes[index] {
            case UInt8(ascii: "n"): try literal("null"); return .null
            case UInt8(ascii: "t"): try literal("true"); return .bool(true)
            case UInt8(ascii: "f"): try literal("false"); return .bool(false)
            case UInt8(ascii: "\""): return .text(try string())
            case UInt8(ascii: "["):
                index += 1
                var items: [JSON] = []
                skipSpace()
                if index < bytes.count, bytes[index] == UInt8(ascii: "]") { index += 1; return .list(items) }
                while true {
                    items.append(try value(depth: depth + 1))
                    skipSpace()
                    guard index < bytes.count else { throw JSONError("unexpected end") }
                    if bytes[index] == UInt8(ascii: ",") { index += 1; continue }
                    if bytes[index] == UInt8(ascii: "]") { index += 1; return .list(items) }
                    throw JSONError("expected , or ]")
                }
            case UInt8(ascii: "{"):
                index += 1
                var fields: [String: JSON] = [:]
                skipSpace()
                if index < bytes.count, bytes[index] == UInt8(ascii: "}") { index += 1; return .object(fields) }
                while true {
                    skipSpace()
                    guard index < bytes.count, bytes[index] == UInt8(ascii: "\"") else { throw JSONError("expected key") }
                    let key = try string()
                    skipSpace()
                    guard index < bytes.count, bytes[index] == UInt8(ascii: ":") else { throw JSONError("expected :") }
                    index += 1
                    guard fields[key] == nil else { throw JSONError("duplicate key") }
                    fields[key] = try value(depth: depth + 1)
                    skipSpace()
                    guard index < bytes.count else { throw JSONError("unexpected end") }
                    if bytes[index] == UInt8(ascii: ",") { index += 1; continue }
                    if bytes[index] == UInt8(ascii: "}") { index += 1; return .object(fields) }
                    throw JSONError("expected , or }")
                }
            case UInt8(ascii: "-"), UInt8(ascii: "0")...UInt8(ascii: "9"):
                let start = index
                if bytes[index] == UInt8(ascii: "-") { index += 1 }
                while index < bytes.count, (UInt8(ascii: "0")...UInt8(ascii: "9")).contains(bytes[index]) { index += 1 }
                if index < bytes.count, [UInt8(ascii: "."), UInt8(ascii: "e"), UInt8(ascii: "E")].contains(bytes[index]) {
                    throw JSONError("floats are not allowed")
                }
                guard let text = String(bytes: bytes[start..<index], encoding: .utf8), let n = Int64(text) else {
                    throw JSONError("invalid integer")
                }
                return .int(n)
            default:
                throw JSONError("unexpected byte")
            }
        }

        mutating func hex4() throws -> UInt32 {
            guard index + 4 <= bytes.count,
                  let text = String(bytes: bytes[index..<index + 4], encoding: .ascii),
                  let n = UInt32(text, radix: 16) else { throw JSONError("invalid escape") }
            index += 4
            return n
        }

        mutating func string() throws -> String {
            index += 1
            var scalars = String.UnicodeScalarView()
            var raw: [UInt8] = []
            func flush() throws {
                if raw.isEmpty { return }
                guard let s = String(bytes: raw, encoding: .utf8) else { throw JSONError("malformed UTF-8") }
                scalars.append(contentsOf: s.unicodeScalars)
                raw.removeAll()
            }
            while index < bytes.count {
                let b = bytes[index]
                if b == UInt8(ascii: "\"") {
                    index += 1
                    try flush()
                    return String(scalars)
                }
                if b < 0x20 { throw JSONError("control character in string") }
                if b == UInt8(ascii: "\\") {
                    try flush()
                    index += 1
                    guard index < bytes.count else { break }
                    let e = bytes[index]
                    index += 1
                    switch e {
                    case UInt8(ascii: "\""): scalars.append("\"")
                    case UInt8(ascii: "\\"): scalars.append("\\")
                    case UInt8(ascii: "/"): scalars.append("/")
                    case UInt8(ascii: "b"): scalars.append("\u{08}")
                    case UInt8(ascii: "f"): scalars.append("\u{0C}")
                    case UInt8(ascii: "n"): scalars.append("\n")
                    case UInt8(ascii: "r"): scalars.append("\r")
                    case UInt8(ascii: "t"): scalars.append("\t")
                    case UInt8(ascii: "u"):
                        var code = try hex4()
                        if (0xD800...0xDBFF).contains(code) {
                            guard index + 2 <= bytes.count, bytes[index] == UInt8(ascii: "\\"), bytes[index + 1] == UInt8(ascii: "u") else {
                                throw JSONError("unpaired surrogate")
                            }
                            index += 2
                            let low = try hex4()
                            guard (0xDC00...0xDFFF).contains(low) else { throw JSONError("unpaired surrogate") }
                            code = 0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00)
                        }
                        guard code != 0, let scalar = Unicode.Scalar(code) else { throw JSONError("invalid scalar") }
                        scalars.append(scalar)
                    default:
                        throw JSONError("invalid escape")
                    }
                    continue
                }
                raw.append(b)
                index += 1
            }
            throw JSONError("unterminated string")
        }
    }
}
