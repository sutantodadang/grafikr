import Foundation
import Grafikr

guard CommandLine.arguments.count == 3 else {
    FileHandle.standardError.write(Data("usage: grafikr-runtime <unpacked-bundle-dir> <absolute-cache-dir>\n".utf8))
    exit(EXIT_FAILURE)
}

do {
    let host = try Runtime.install(
        bundle: URL(fileURLWithPath: CommandLine.arguments[1]),
        cache: URL(fileURLWithPath: CommandLine.arguments[2])
    )
    print(host.path)
} catch {
    FileHandle.standardError.write(Data("grafikr-runtime: \(error)\n".utf8))
    exit(EXIT_FAILURE)
}
