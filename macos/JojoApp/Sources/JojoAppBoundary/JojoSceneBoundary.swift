import SwiftUI

/// Compile-checked native scene boundary. Concrete scenes, windows, lifecycle and IPC
/// deliberately wait for P1-14/P1-13; this package is not a runnable app bundle.
@MainActor
public protocol JojoSceneBoundary: Scene {}
