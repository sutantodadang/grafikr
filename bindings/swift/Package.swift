// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "Grafikr",
    products: [
        .library(name: "Grafikr", targets: ["Grafikr"]),
        .executable(name: "grafikr-runtime", targets: ["grafikr-runtime"]),
    ],
    dependencies: [.package(url: "https://github.com/apple/swift-crypto.git", from: "3.12.3")],
    targets: [
        .target(name: "Grafikr", dependencies: [.product(name: "Crypto", package: "swift-crypto")]),
        .executableTarget(name: "grafikr-runtime", dependencies: ["Grafikr"]),
        .testTarget(name: "GrafikrTests", dependencies: ["Grafikr"]),
    ]
)
