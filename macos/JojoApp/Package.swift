// swift-tools-version: 6.0
import PackageDescription

// Dependency-free. JojoCore is Foundation-only transport/lifecycle logic; JojoApp is the
// native shell (SwiftUI content, AppKit termination/window policy). No package resolution.
let package = Package(
    name: "JojoAppBoundary",
    platforms: [.macOS(.v14)],
    products: [
        .library(name: "JojoAppBoundary", targets: ["JojoAppBoundary"]),
        .executable(name: "JojoApp", targets: ["JojoApp"]),
    ],
    targets: [
        .target(name: "JojoAppBoundary"),
        .target(name: "JojoCore"),
        .executableTarget(name: "JojoApp", dependencies: ["JojoCore", "JojoAppBoundary"]),
        // One test target: the gate reads the summary of a single test bundle.
        .testTarget(name: "JojoCoreTests", dependencies: ["JojoCore", "JojoApp"]),
    ]
)
