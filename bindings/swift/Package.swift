// swift-tools-version: 6.0
import PackageDescription

#if os(Windows)
let packageDependencies: [Package.Dependency] = []
let grafikrDependencies: [Target.Dependency] = []
#else
let packageDependencies: [Package.Dependency] = [.package(url: "https://github.com/apple/swift-crypto.git", from: "3.12.3")]
let grafikrDependencies: [Target.Dependency] = [.product(name: "Crypto", package: "swift-crypto")]
#endif

let package = Package(
    name: "Grafikr",
    products: [
        .library(name: "Grafikr", targets: ["Grafikr"]),
        .executable(name: "grafikr-runtime", targets: ["grafikr-runtime"]),
    ],
    dependencies: packageDependencies,
    targets: [
        .target(
            name: "Grafikr",
            dependencies: grafikrDependencies,
            linkerSettings: [.linkedLibrary("bcrypt", .when(platforms: [.windows]))]
        ),
        .executableTarget(name: "grafikr-runtime", dependencies: ["Grafikr"]),
        .testTarget(name: "GrafikrTests", dependencies: ["Grafikr"]),
    ]
)
