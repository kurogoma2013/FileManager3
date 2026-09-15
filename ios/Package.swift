// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "FileManager3IOS",
    platforms: [.iOS(.v17)],
    products: [.library(name: "FileManager3IOS", targets: ["FileManager3IOS"])],
    targets: [.target(name: "FileManager3IOS")]
)
