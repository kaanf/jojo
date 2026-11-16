import Foundation

/// Run-loop timers in common modes, so they keep firing while AppKit waits for a deferred
/// termination reply (modal-panel run-loop mode).
@MainActor
public final class TimerScheduler: QuitScheduler {
    final class Handle: QuitTimer {
        var timer: Timer?
        func cancel() {
            timer?.invalidate()
            timer = nil
        }
    }

    public init() {}

    public func after(_ seconds: Double, _ action: @escaping @MainActor () -> Void) -> QuitTimer {
        let handle = Handle()
        let timer = Timer(timeInterval: seconds, repeats: false) { _ in
            MainActor.assumeIsolated { action() }
        }
        RunLoop.main.add(timer, forMode: .common)
        handle.timer = timer
        return handle
    }
}

/// Runs `block` on the main run loop in common modes. Unlike DispatchQueue.main, run-loop
/// blocks are serviced inside nested run loops, so helper frames keep arriving while AppKit
/// waits for a deferred termination reply that was started from another main-thread block.
public func onMainRunLoop(_ block: @escaping @MainActor () -> Void) {
    let main = CFRunLoopGetMain()
    CFRunLoopPerformBlock(main, CFRunLoopMode.commonModes.rawValue) { MainActor.assumeIsolated(block) }
    CFRunLoopWakeUp(main)
}
