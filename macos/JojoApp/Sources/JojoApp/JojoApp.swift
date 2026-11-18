import JojoAppBoundary
import SwiftUI

/// SwiftUI application. Primary and auxiliary windows are AppKit-owned hosts for SwiftUI
/// content so the delegate can route last-primary-window close through the quit coordinator.
@main
struct JojoApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate

    var body: some Scene {
        Settings {
            Text("Jojo's settings are in the main window, under Settings & Providers.").padding()
        }
        .commands {
            // Replaces AppKit's terminate: menu item so Cmd-Q always reaches the coordinator.
            CommandGroup(replacing: .appTermination) {
                Button("Quit Jojo") { delegate.requestQuit(.quitCommand) }
                    .keyboardShortcut("q", modifiers: [.command])
            }
            CommandGroup(after: .windowArrangement) {
                Button("Show Jojo") { delegate.showPrimary() }
                    .keyboardShortcut("0", modifiers: [.command])
                Button("Show Inspector") { delegate.showAuxiliary() }
                    .keyboardShortcut("i", modifiers: [.command, .option])
            }
        }
    }
}
