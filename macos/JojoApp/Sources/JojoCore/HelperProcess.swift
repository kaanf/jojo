import Foundation

/// What the GUI observes about its helper. `ended` is delivered only after both helper
/// stdout EOF and process exit, so every frame (including ShutdownReady) is seen first.
public enum TransportEvent: Sendable, Equatable {
    case frame([UInt8])
    case channelFailed(String)
    case ended(status: Int32)
}

/// The only GUI → helper surface: bytes in, frames and exit status out. It has no way to
/// signal, interrupt or terminate a process; stopping work belongs to the Rust runner.
@MainActor
public protocol HelperTransport: AnyObject {
    var onEvent: ((TransportEvent) -> Void)? { get set }
    var processIdentifier: Int32 { get }
    func start() throws
    func send(_ frame: [UInt8])
}

/// Launches exactly `jojo-helper --serve` over private pipes with an explicit environment.
/// The GUI's lifetime holds the helper's stdin: if the GUI dies, the helper sees EOF and
/// runs its existing emergency stop. Nothing here kills the helper.
@MainActor
public final class ProcessHelperTransport: HelperTransport {
    public struct Configuration: Sendable {
        public var executable: URL
        public var dataRoot: String?
        public var worktreeRoot: String?
        public var home: String?
        /// P2-02 slice options (`--disposable-root`, `--codex-…`), passed through verbatim
        /// from whoever launched the app. The GUI never composes or edits them.
        public var slice: [String]
        public init(executable: URL, dataRoot: String? = nil, worktreeRoot: String? = nil, home: String? = nil,
                    slice: [String] = []) {
            self.executable = executable
            self.dataRoot = dataRoot
            self.worktreeRoot = worktreeRoot
            self.home = home
            self.slice = slice
        }
    }

    public var onEvent: ((TransportEvent) -> Void)?
    public private(set) var stderrTail = ""
    private let configuration: Configuration
    private let process = Process()
    private let input = Pipe()
    private let output = Pipe()
    private let errors = Pipe()
    private let writer = DispatchQueue(label: "jojo.helper.stdin")
    private var splitter = FrameSplitter()
    private var exitStatus: Int32?
    private var outputClosed = false
    private var channelOK = true
    private var ended = false

    public init(_ configuration: Configuration) {
        self.configuration = configuration
    }

    public var processIdentifier: Int32 { process.processIdentifier }

    public func start() throws {
        // A write after helper exit must surface as an error, not kill the GUI.
        signal(SIGPIPE, SIG_IGN)
        process.executableURL = configuration.executable
        var arguments = ["--serve"]
        if let data = configuration.dataRoot { arguments += ["--data-root", data] }
        if let worktrees = configuration.worktreeRoot { arguments += ["--worktree-root", worktrees] }
        process.arguments = arguments + configuration.slice
        // No inherited credentials or PATH: HOME only, for the accepted default roots.
        process.environment = configuration.home.map { ["HOME": $0] } ?? [:]
        process.standardInput = input
        process.standardOutput = output
        process.standardError = errors
        output.fileHandleForReading.readabilityHandler = { [weak self] handle in
            let data = handle.availableData
            if data.isEmpty { handle.readabilityHandler = nil }
            onMainRunLoop { self?.stdout(data) }
        }
        errors.fileHandleForReading.readabilityHandler = { [weak self] handle in
            let data = handle.availableData
            if data.isEmpty { handle.readabilityHandler = nil }
            onMainRunLoop { self?.stderr(data) }
        }
        process.terminationHandler = { [weak self] p in
            let status = p.terminationReason == .exit ? p.terminationStatus : -p.terminationStatus
            onMainRunLoop { self?.exited(status) }
        }
        try process.run()
    }

    public func send(_ frame: [UInt8]) {
        guard channelOK else { return }
        let handle = input.fileHandleForWriting
        writer.async { [weak self] in
            do {
                try handle.write(contentsOf: Data(frame))
            } catch {
                onMainRunLoop { self?.fail("helper stdin write failed") }
            }
        }
    }

    private func stdout(_ data: Data) {
        if data.isEmpty {
            outputClosed = true
            if channelOK, (try? splitter.finish()) == nil { fail("truncated helper frame") }
            finishIfDone()
            return
        }
        guard channelOK else { return }
        do {
            for frame in try splitter.push(data) { onEvent?(.frame(frame)) }
        } catch {
            fail("helper framing error: \(error)")
        }
    }

    private func stderr(_ data: Data) {
        stderrTail = String((stderrTail + String(decoding: data, as: UTF8.self)).suffix(4096))
    }

    private func exited(_ status: Int32) {
        exitStatus = status
        finishIfDone()
    }

    private func fail(_ reason: String) {
        guard channelOK else { return }
        channelOK = false
        onEvent?(.channelFailed(reason))
    }

    private func finishIfDone() {
        guard !ended, outputClosed, let status = exitStatus else { return }
        ended = true
        onEvent?(.ended(status: status))
    }
}
