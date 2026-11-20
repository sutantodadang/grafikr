import Foundation
import Grafikr

@main
struct CounterExample {
    static func main() async throws {
        guard let hostPath = CommandLine.arguments.dropFirst().first else {
            fatalError("usage: CounterExample <absolute-grafikr-host-path>")
        }

        let app = try App(host: URL(fileURLWithPath: hostPath))
        do {
            try await app.start()
            let window = try await app.createWindow(title: "Grafikr Swift", width: 480, height: 320)
            let button = try await app.createNode(
                window: window,
                widget: "Button",
                properties: ["label": "Click me"]
            )
            let subscription = try await app.on(node: button.handle, event: "click") { _ in
                print("Button clicked")
            }

            print("Window is open. Press Enter to close it.")
            _ = readLine()
            await subscription.close()
            await app.close()
        } catch {
            await app.close()
            throw error
        }
    }
}
