import AppKit
import JojoCore
import SwiftUI

struct LaunchOptions {
    var helper: URL?
    var dataRoot: String?
    var worktreeRoot: String?
    var trace: String?
    var script: String?
    /// P2-02 slice options and the M1 Claude Code options, forwarded to the helper exactly
    /// as given at launch.
    var slice: [String] = []
    static let sliceOptions: Set<String> = ["--disposable-root", "--codex-executable", "--codex-version",
                                            "--codex-sha256", "--codex-home", "--codex-auth",
                                            "--claude-executable", "--claude-version", "--claude-sha256",
                                            "--claude-home", "--claude-user", "--claude-auth", "--login-shell"]

    static func parse(_ arguments: [String]) -> LaunchOptions {
        var o = LaunchOptions()
        var i = 1
        while i + 1 < arguments.count {
            let value = arguments[i + 1]
            switch arguments[i] {
            case "--helper": o.helper = URL(fileURLWithPath: value)
            case "--data-root": o.dataRoot = value
            case "--worktree-root": o.worktreeRoot = value
            case "--trace": o.trace = value
            case "--ui-script": o.script = value
            case let name where sliceOptions.contains(name): o.slice += [name, value]
            default: i -= 1
            }
            i += 2
        }
        // D3: the core asks the user's login shell for PATH; pass the shell this app was
        // started under unless the launch named one. Only a slice configuration uses it.
        if o.slice.contains("--disposable-root"), !o.slice.contains("--login-shell"),
           let shell = ProcessInfo.processInfo.environment["SHELL"], shell.hasPrefix("/") {
            o.slice += ["--login-shell", shell]
        }
        // Development/packaging default: helper beside the GUI executable.
        if o.helper == nil, let exe = Bundle.main.executableURL {
            o.helper = exe.deletingLastPathComponent().appendingPathComponent("jojo-helper")
        }
        return o
    }
}

/// Append-only JSONL lifecycle trace for runtime GUI verification (opt-in via --trace).
@MainActor
final class Tracer {
    private let url: URL?
    private var lines: [String] = []
    private let start = Date()

    init(_ path: String?) {
        url = path.map(URL.init(fileURLWithPath:))
    }

    func callAsFunction(_ event: String) {
        guard let url else { return }
        let ms = Int(Date().timeIntervalSince(start) * 1000)
        lines.append(StrictJSON.encode(.object(["ms": .int(Int64(ms)), "event": .text(event)])))
        try? Data((lines.joined(separator: "\n") + "\n").utf8).write(to: url, options: .atomic)
    }
}

@MainActor
final class AppModel: ObservableObject {
    let client: HelperClient
    let coordinator: QuitCoordinator
    let workspace: WorkspaceModel
    let trace: Tracer
    let transport: ProcessHelperTransport?
    /// What the app was started with; Settings & Providers shows what it passed to the helper.
    let options: LaunchOptions
    /// The delegate's single quit entry (set at launch).
    var quit: (QuitSource) -> Void = { _ in }

    init(options: LaunchOptions, trace: Tracer) {
        self.trace = trace
        self.options = options
        let transport = options.helper.map {
            ProcessHelperTransport(.init(executable: $0, dataRoot: options.dataRoot, worktreeRoot: options.worktreeRoot,
                                         home: ProcessInfo.processInfo.environment["HOME"], slice: options.slice))
        }
        self.transport = transport
        client = HelperClient(transport: transport ?? MissingHelper())
        coordinator = QuitCoordinator(client: client, scheduler: TimerScheduler())
        workspace = WorkspaceModel(client: client)
        client.trace = { trace("client \($0)") }
        coordinator.trace = { trace("quit \($0)") }
        coordinator.onChange = { [weak self] in self?.objectWillChange.send() }
        workspace.onChange = { [weak self] in self?.objectWillChange.send() }
        client.onConnectionChange = { [weak self] connection in
            guard let self else { return }
            trace("connection \(connection)")
            if case .ready = connection {
                trace("helper-pid \(self.client.processIdentifier)")
                self.workspace.refresh()
            }
            self.coordinator.connectionChanged(connection)
            self.objectWillChange.send()
        }
    }

    func start() {
        client.start()
    }
}

/// Used when no helper executable is configured; launch fails visibly.
@MainActor
final class MissingHelper: HelperTransport {
    var onEvent: ((TransportEvent) -> Void)?
    var processIdentifier: Int32 { 0 }
    func start() throws { throw JSONError("no helper executable configured") }
    func send(_ frame: [UInt8]) {}
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate, NSWindowDelegate {
    private(set) var model: AppModel!
    private(set) var primary: NSWindow?
    private(set) var auxiliary: NSWindow?
    private var pendingSource = QuitSource.quitCommand
    private var terminationScheduled = false
    private var driver: UITestDriver?
    private var eventTimer: Timer?

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.regular)
        // Never let the OS silently kill Jojo while a helper session may own agents.
        ProcessInfo.processInfo.disableSuddenTermination()
        ProcessInfo.processInfo.disableAutomaticTermination("Jojo helper session")
        let options = LaunchOptions.parse(CommandLine.arguments)
        let trace = Tracer(options.trace)
        trace("launched gui-pid \(ProcessInfo.processInfo.processIdentifier)")
        model = AppModel(options: options, trace: trace)
        model.coordinator.reply = { NSApp.reply(toApplicationShouldTerminate: $0) }
        model.coordinator.requestTermination = { onMainRunLoop { NSApp.terminate(nil) } }
        model.coordinator.onAttention = { [weak self] in self?.showPrimary() }
        model.quit = { [weak self] in self?.requestQuit($0) }
        // Dock Quit, logout and scripted quits arrive as this Apple event; route them too.
        NSAppleEventManager.shared().setEventHandler(self, andSelector: #selector(quitEvent(_:reply:)),
                                                     forEventClass: AEEventClass(kCoreEventClass),
                                                     andEventID: AEEventID(kAEQuitApplication))
        showPrimary()
        model.start()
        let timer = Timer(timeInterval: 2, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.model.workspace.pollEvents() }
        }
        RunLoop.main.add(timer, forMode: .common)
        eventTimer = timer
        if let script = options.script {
            driver = UITestDriver(script: script, delegate: self, trace: trace)
            driver?.start()
        }
        NSApp.activate()
    }

    /// Every quit path arrives here. Shutdown starts in this decision hook, never in
    /// applicationWillTerminate (too late, and skipped by sudden termination).
    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        let source = pendingSource
        pendingSource = .quitCommand
        terminationScheduled = false
        let decision = model.coordinator.requestQuit(source)
        model.trace("terminate-decision \(source.rawValue) \(decision)")
        switch decision {
        case .now: return .terminateNow
        case .later: return .terminateLater
        case .cancel: return .terminateCancel
        }
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        false
    }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        showPrimary()
        return true
    }

    func applicationWillTerminate(_ notification: Notification) {
        model.trace("will-terminate outcome=\(model.coordinator.outcome?.label ?? "none") \(String(describing: model.coordinator.outcome))")
    }

    func windowShouldClose(_ sender: NSWindow) -> Bool {
        let role: WindowRole = sender === primary ? .primary : .auxiliary
        switch WindowPolicy.onClose(role) {
        case .close:
            model.trace("window-close \(role.rawValue)")
            return true
        case .routeToQuit:
            model.trace("window-close \(role.rawValue) routed-to-quit")
            requestQuit(.lastPrimaryWindow)
            return false
        }
    }

    /// The one quit entry for Cmd-Q/menu Quit, the quit Apple event and last-primary-window
    /// close. AppKit terminates without asking the delegate if `terminate` is re-entered while
    /// a deferred reply is pending, so duplicates never reach AppKit: they join the operation.
    func requestQuit(_ source: QuitSource) {
        if terminationScheduled {
            model.trace("duplicate-quit \(source.rawValue) before decision")
            return
        }
        if model.coordinator.phase.inProgress {
            let decision = model.coordinator.requestQuit(source)
            model.trace("joined-quit \(source.rawValue) \(decision)")
            return
        }
        terminationScheduled = true
        pendingSource = source
        onMainRunLoop { NSApp.terminate(nil) }
    }

    @objc private func quitEvent(_ event: NSAppleEventDescriptor, reply: NSAppleEventDescriptor) {
        model.trace("quit-apple-event")
        requestQuit(.quitCommand)
    }

    func showPrimary() {
        if primary == nil {
            let window = makeWindow(title: "Jojo", size: NSSize(width: 1120, height: 720),
                                    content: MainView(model: model, delegate: self))
            // The shell draws its own toolbar strip and sidebar under the system title bar;
            // traffic lights, window shadow and corners stay the system's.
            window.styleMask.insert(.fullSizeContentView)
            window.titlebarAppearsTransparent = true
            window.titleVisibility = .hidden
            // An empty unified toolbar gives the title bar the macOS 26 height, which places
            // the traffic lights inside the sidebar's glass panel instead of on its corner.
            window.toolbar = NSToolbar(identifier: "jojo.primary")
            window.toolbarStyle = .unified
            window.identifier = NSUserInterfaceItemIdentifier("jojo.primary")
            primary = window
        }
        primary?.makeKeyAndOrderFront(nil)
    }

    func showAuxiliary() {
        if auxiliary == nil {
            let window = makeWindow(title: "Jojo Inspector", size: NSSize(width: 520, height: 420),
                                    content: InspectorView(model: model))
            window.identifier = NSUserInterfaceItemIdentifier("jojo.auxiliary")
            auxiliary = window
        }
        auxiliary?.makeKeyAndOrderFront(nil)
        model.trace("window-open auxiliary")
    }

    func windowWillClose(_ notification: Notification) {
        if (notification.object as? NSWindow) === auxiliary { auxiliary = nil }
    }

    private func makeWindow(title: String, size: NSSize, content: some View) -> NSWindow {
        let window = NSWindow(contentRect: NSRect(origin: .zero, size: size),
                              styleMask: [.titled, .closable, .miniaturizable, .resizable],
                              backing: .buffered, defer: false)
        window.title = title
        window.isReleasedWhenClosed = false
        window.contentView = NSHostingView(rootView: content)
        window.delegate = self
        window.center()
        return window
    }
}
