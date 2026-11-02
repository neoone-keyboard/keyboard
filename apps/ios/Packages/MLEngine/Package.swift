// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "MLEngine",
    platforms: [.iOS("26.0"), .macOS("26.0")],
    products: [.library(name: "MLEngine", targets: ["MLEngine"])],
    targets: [
        .binaryTarget(name: "MLEngineFFI", path: "MLEngineFFI.xcframework"),
        .target(
            name: "MLEngine",
            dependencies: ["MLEngineFFI"],
            // UniFFI-generated code is not written for Swift 6 strict concurrency.
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
        .testTarget(name: "MLEngineTests", dependencies: ["MLEngine"]),
    ]
)
