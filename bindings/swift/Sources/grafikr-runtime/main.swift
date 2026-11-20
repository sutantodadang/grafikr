import Foundation
import Grafikr

guard CommandLine.arguments.count == 3 else {
    fputs("usage: grafikr-runtime <unpacked-bundle-dir> <absolute-cache-dir>\n", stderr)
    exit(EXIT_FAILURE)
}

do {
    let host = try Runtime.install(
        bundle: URL(fileURLWithPath: CommandLine.arguments[1]),
        cache: URL(fileURLWithPath: CommandLine.arguments[2])
    )
    print(host.path)
} catch {
    fputs("grafikr-runtime: \(error)\n", stderr)
    exit(EXIT_FAILURE)
}
