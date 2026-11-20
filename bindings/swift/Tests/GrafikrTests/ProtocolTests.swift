import XCTest
@testable import Grafikr

private actor RequestResults {
    private var codes: [String] = []
    func append(_ code: String) { codes.append(code) }
    func count(_ code: String) -> Int { codes.filter { $0 == code }.count }
}

final class ProtocolTests: XCTestCase {
    func testFrameLimit() { XCTAssertEqual(App.maxFrame, 8 * 1024 * 1024) }
    func testOutstandingRequestLimit() { XCTAssertEqual(App.maxOutstandingRequests, 256) }

    func testRejectsRequestsBeyondLimitAndCloseReleasesPendingRequests() async throws {
        #if os(Windows)
        let fixture = "../../target/debug/fixture-host.exe"
        #else
        let fixture = "../../target/debug/fixture-host"
        #endif
        let host = URL(fileURLWithPath: ProcessInfo.processInfo.environment["GRAFIKR_FIXTURE_HOST"] ?? fixture, relativeTo: URL(fileURLWithPath: FileManager.default.currentDirectoryPath)).standardizedFileURL
        var environment = ProcessInfo.processInfo.environment
        environment["GRAFIKR_FIXTURE_HOLD_RESPONSES"] = "1"
        let app = try App(host: host, environment: environment)
        try await app.start()

        let results = RequestResults()
        let queueFull = expectation(description: "request beyond cap is rejected")
        let completed = expectation(description: "all request tasks finish")
        completed.expectedFulfillmentCount = App.maxOutstandingRequests + 1
        let tasks = (0...App.maxOutstandingRequests).map { _ in
            Task {
                do {
                    _ = try await app.request("state.get", ["state":"0"])
                    await results.append("success")
                } catch let error as BridgeError {
                    await results.append(error.code)
                    if error.code == "queue_full" { queueFull.fulfill() }
                } catch {
                    await results.append("unexpected")
                }
                completed.fulfill()
            }
        }

        await fulfillment(of: [queueFull], timeout: 2)
        await app.close()
        await fulfillment(of: [completed], timeout: 2)
        for task in tasks { await task.value }
        let rejected = await results.count("queue_full")
        let released = await results.count("connection_closed")
        XCTAssertEqual(rejected, 1)
        XCTAssertEqual(released, App.maxOutstandingRequests)
    }

    func testDeliversEventSentBeforeSubscribeResponse() async throws {
        #if os(Windows)
        let fixture = "../../target/debug/fixture-host.exe"
        #else
        let fixture = "../../target/debug/fixture-host"
        #endif
        let host = URL(fileURLWithPath: ProcessInfo.processInfo.environment["GRAFIKR_FIXTURE_HOST"] ?? fixture, relativeTo: URL(fileURLWithPath: FileManager.default.currentDirectoryPath)).standardizedFileURL
        var environment = ProcessInfo.processInfo.environment
        environment["GRAFIKR_FIXTURE_EARLY_SUBSCRIBE"] = "1"
        let app = try App(host: host, environment: environment)
        try await app.start()
        let window = try await app.createWindow(title: "fixture", width: 320, height: 240)
        let button = try await app.createNode(window: window, widget: "Button", properties: ["label":"click"])
        try await button.attach(to: window.root)
        let received = expectation(description: "early event")
        let subscription = try await app.on(node: button.handle, event: "click") { event in
            XCTAssertEqual(event.source, button.handle)
            XCTAssertEqual(event.name, "click")
            received.fulfill()
        }
        _ = subscription
        await fulfillment(of: [received], timeout: 2)
        await app.close()
    }

    func testConcurrentRequestsCorrelateOutOfOrderResponses() async throws {
        #if os(Windows)
        let fixture = "../../target/debug/fixture-host.exe"
        #else
        let fixture = "../../target/debug/fixture-host"
        #endif
        let host = URL(fileURLWithPath: ProcessInfo.processInfo.environment["GRAFIKR_FIXTURE_HOST"] ?? fixture, relativeTo: URL(fileURLWithPath: FileManager.default.currentDirectoryPath)).standardizedFileURL
        var environment = ProcessInfo.processInfo.environment
        environment["GRAFIKR_FIXTURE_REVERSE_RESPONSES"] = "1"
        let app = try App(host: host, environment: environment)
        try await app.start()
        let state = try await app.createState(type: "bool", value: true)
        async let first = state.get()
        async let second = state.get()
        let (firstResult, secondResult) = try await (first, second)
        XCTAssertFalse(firstResult.isEmpty)
        XCTAssertFalse(secondResult.isEmpty)
        await app.close()
    }

    func testUtf8AndNewlinePayloadWithinFrameLimit() async throws {
        #if os(Windows)
        let fixture = "../../target/debug/fixture-host.exe"
        #else
        let fixture = "../../target/debug/fixture-host"
        #endif
        let host = URL(fileURLWithPath: ProcessInfo.processInfo.environment["GRAFIKR_FIXTURE_HOST"] ?? fixture, relativeTo: URL(fileURLWithPath: FileManager.default.currentDirectoryPath)).standardizedFileURL
        let app = try App(host: host)
        try await app.start()
        let title = String(repeating: "A\n界é", count: 20_000)
        let window = try await app.createWindow(title: title, width: 320, height: 240)
        XCTAssertFalse(window.handle.isEmpty)
        await app.close()
    }

    func testInt64BoundariesRoundTripAsDecimalStrings() async throws {
        #if os(Windows)
        let fixture = "../../target/debug/fixture-host.exe"
        #else
        let fixture = "../../target/debug/fixture-host"
        #endif
        let host = URL(fileURLWithPath: ProcessInfo.processInfo.environment["GRAFIKR_FIXTURE_HOST"] ?? fixture, relativeTo: URL(fileURLWithPath: FileManager.default.currentDirectoryPath)).standardizedFileURL
        let app = try App(host: host)
        try await app.start()
        let state = try await app.createState(type: "i64", value: "-9223372036854775808")
        let lowerData = try await state.get()
        let lower = try JSONSerialization.jsonObject(with: lowerData) as? [String: Any]
        XCTAssertEqual(lower?["value"] as? String, "-9223372036854775808")
        try await state.set(type: "i64", value: "9223372036854775807")
        let upperData = try await state.get()
        let upper = try JSONSerialization.jsonObject(with: upperData) as? [String: Any]
        XCTAssertEqual(upper?["value"] as? String, "9223372036854775807")
        await app.close()
    }

    func testNonFiniteNumberIsRejectedBeforeWriting() async throws {
        #if os(Windows)
        let fixture = "../../target/debug/fixture-host.exe"
        #else
        let fixture = "../../target/debug/fixture-host"
        #endif
        let host = URL(fileURLWithPath: ProcessInfo.processInfo.environment["GRAFIKR_FIXTURE_HOST"] ?? fixture, relativeTo: URL(fileURLWithPath: FileManager.default.currentDirectoryPath)).standardizedFileURL
        let app = try App(host: host)
        try await app.start()
        do {
            _ = try await app.createState(type: "f64", value: Double.nan)
            XCTFail("non-finite value was accepted")
        } catch { }
        await app.close()
    }

    func testClosingAppDoesNotTerminateUnrelatedProcess() async throws {
        #if os(Windows)
        let fixture = "../../target/debug/fixture-host.exe"
        #else
        let fixture = "../../target/debug/fixture-host"
        #endif
        let host = URL(fileURLWithPath: ProcessInfo.processInfo.environment["GRAFIKR_FIXTURE_HOST"] ?? fixture, relativeTo: URL(fileURLWithPath: FileManager.default.currentDirectoryPath)).standardizedFileURL
        let sentinel = Process()
        sentinel.executableURL = host
        sentinel.arguments = ["--sleep-ms", "15000"]
        sentinel.standardOutput = FileHandle.nullDevice
        sentinel.standardError = FileHandle.nullDevice
        try sentinel.run()
        let app = try App(host: host)
        try await app.start()
        await app.close()
        XCTAssertTrue(sentinel.isRunning)
        sentinel.terminate()
        sentinel.waitUntilExit()
    }

    func testWidgetPropertiesAndEventsMatchProtocolInventory() async throws {
        #if os(Windows)
        let fixture = "../../target/debug/fixture-host.exe"
        #else
        let fixture = "../../target/debug/fixture-host"
        #endif
        let host = URL(fileURLWithPath: ProcessInfo.processInfo.environment["GRAFIKR_FIXTURE_HOST"] ?? fixture, relativeTo: URL(fileURLWithPath: FileManager.default.currentDirectoryPath)).standardizedFileURL
        var environment = ProcessInfo.processInfo.environment
        environment["GRAFIKR_FIXTURE_EARLY_SUBSCRIBE"] = "1"
        let app = try App(host: host, environment: environment)
        try await app.start()
        let window = try await app.createWindow(title: "widget contract", width: 800, height: 600)
        _ = try await app.request("window.update", ["window":window.handle,"title":"widget contract updated","width":820,"height":620,"theme":"dark"])
        let group = try await app.createState(type: "i64", value: "0")

        let uploadData = try await app.request("image.begin", ["width":1,"height":1,"byte_length":4])
        let upload = try XCTUnwrap((try JSONSerialization.jsonObject(with: uploadData) as? [String: String])?["upload"])
        _ = try await app.request("image.chunk", ["upload":upload,"offset":0,"data":"AAD/AA=="])
        let imageData = try await app.request("image.finalize", ["upload":upload])
        let image = try XCTUnwrap((try JSONSerialization.jsonObject(with: imageData) as? [String: String])?["image"])

        let transform: [String: Any] = ["x":1,"y":2,"scale":1]
        let roots: [[String: Any]] = [["key":"root","label":"Root","children":[["key":"leaf","label":"Leaf","children":[]]]]]
        let widgets: [(String, [String: Any], [String], [String: Any])] = [
            ("Column",["gap":2,"padding":4,"background":"#336699","radius":3,"width":200,"transform":transform],[],["gap":3]),
            ("Row",["gap":2,"padding":4,"background":"#336699","radius":3,"height":40,"transform":transform],[],["gap":3]),
            ("Grid",["columns":2,"gap":2,"padding":4,"width":200,"transform":transform],[],["columns":3]),
            ("Stack",["width":200,"height":100,"transform":transform],[],["width":220]),
            ("Text",["text":"hello","size":16,"color":"#336699"],[],["text":"updated"]),
            ("Button",["label":"button","variant":"secondary"],["click"],["label":"updated"]),
            ("TextField",["text":"input","width":180,"multiline":true,"password":false],["change"],["text":"updated"]),
            ("Checkbox",["label":"check","checked":false],["change"],["checked":true]),
            ("Radio",["label":"radio","group":group.handle,"value":0],["change"],["value":1]),
            ("Switch",["label":"switch","checked":false],["change"],["checked":true]),
            ("Slider",["value":0.5,"min":0,"max":1,"step":0.1,"width":180],["change"],["value":0.6]),
            ("ProgressBar",["value":0.5,"width":180],[],["value":0.6]),
            ("Spinner",["size":20,"label":"loading"],[],["label":"working"]),
            ("ScrollView",["width":200,"height":100,"grow":false],[],["grow":true]),
            ("VirtualList",["row_count":10,"row_height":24,"width":200,"height":120],["select","data.request"],[:]),
            ("DataGrid",["columns":[["title":"Name","width":120]],"row_count":10,"row_height":24,"width":240,"height":120,"selected":0],["select","data.request"],[:]),
            ("TreeView",["roots":roots,"width":200,"height":120,"selected":"leaf"],["select"],["selected":"root"]),
            ("Modal",["open":false,"width":320],[],["open":true]),
            ("Popup",["open":false,"x":10,"y":20,"width":180],[],["open":true]),
            ("Tabs",["labels":["One","Two"],"active":0,"width":300,"height":180],["change"],["active":1]),
            ("ComboBox",["options":["One","Two"],"selected":0,"width":180],["select"],["selected":1]),
            ("Tooltip",["label":"help","delay_ms":450],[],["label":"updated"]),
            ("Image",["image":image,"width":32,"height":32,"label":"sample"],[],["label":"updated"]),
            ("Canvas",["commands":[],"label":"drawing","width":120,"height":80],["resize"],["label":"updated"]),
        ]
        var subscriptions: [Subscription] = []
        let receivedEvents = expectation(description: "widget events arrive")
        let collectionRequestsAreFixtureDriven = environment["GRAFIKR_FIXTURE_COLLECTION_REQUEST"] != nil
        receivedEvents.expectedFulfillmentCount = widgets.reduce(0) { count, entry in
            count + entry.2.filter { $0 != "data.request" || !collectionRequestsAreFixtureDriven }.count
        }
        for (widget, properties, events, updates) in widgets {
            let node: Node
            do { node = try await app.createNode(window: window, widget: widget, properties: properties) }
            catch let error as BridgeError { throw BridgeError(code:error.code,message:"\(widget): \(error.message)") }
            if !updates.isEmpty {
                do { try await node.update(updates) }
                catch let error as BridgeError { throw BridgeError(code:error.code,message:"\(widget) update: \(error.message)") }
            }
            if widget == "Canvas" {
                _ = try await app.request("canvas.update", ["node":node.handle,"commands":[["kind":"rect","rect":["x":0,"y":0,"width":20,"height":10],"color":"#336699","radius":0]]])
            }
            for event in events {
                do { subscriptions.append(try await app.on(node: node.handle, event: event) { _ in receivedEvents.fulfill() }) }
                catch let error as BridgeError { throw BridgeError(code:error.code,message:"\(widget) event \(event): \(error.message)") }
            }
        }
        await fulfillment(of: [receivedEvents], timeout: 4)
        for subscription in subscriptions { await subscription.close() }
        _ = try await app.request("image.destroy", ["image":image])
        try await window.close()
        await app.close()
    }

    func testCallbackCanSubmitNestedRequest() async throws {
        #if os(Windows)
        let fixture = "../../target/debug/fixture-host.exe"
        #else
        let fixture = "../../target/debug/fixture-host"
        #endif
        let host = URL(fileURLWithPath: ProcessInfo.processInfo.environment["GRAFIKR_FIXTURE_HOST"] ?? fixture, relativeTo: URL(fileURLWithPath: FileManager.default.currentDirectoryPath)).standardizedFileURL
        var environment = ProcessInfo.processInfo.environment
        environment["GRAFIKR_FIXTURE_EARLY_SUBSCRIBE"] = "1"
        let app = try App(host: host, environment: environment)
        try await app.start()
        let window = try await app.createWindow(title: "nested request", width: 320, height: 240)
        let button = try await app.createNode(window: window, widget: "Button", properties: ["label":"nested"])
        try await button.attach(to: window.root)
        let state = try await app.createState(type: "bool", value: true)
        let completed = expectation(description: "callback request completes")
        let subscription = try await app.on(node: button.handle, event: "click") { _ in
            Task {
                do { _ = try await app.request("state.get", ["state":state.handle]) }
                catch { XCTFail("nested request failed: " + String(describing:error)) }
                completed.fulfill()
            }
        }
        await fulfillment(of: [completed], timeout: 3)
        await subscription.close()
        await app.close()
    }

    func testThrowingEventHandlerIsReportedAndDoesNotStopQueue() async throws {
        #if os(Windows)
        let fixture = "../../target/debug/fixture-host.exe"
        #else
        let fixture = "../../target/debug/fixture-host"
        #endif
        let host = URL(fileURLWithPath: ProcessInfo.processInfo.environment["GRAFIKR_FIXTURE_HOST"] ?? fixture, relativeTo: URL(fileURLWithPath: FileManager.default.currentDirectoryPath)).standardizedFileURL
        var environment = ProcessInfo.processInfo.environment
        environment["GRAFIKR_FIXTURE_EVENT_BURST"] = "2"
        let app = try App(host: host, environment: environment)
        try await app.start()
        let window = try await app.createWindow(title: "callback errors", width: 320, height: 240)
        let field = try await app.createNode(window: window, widget: "TextField", properties: ["text":"test"])
        try await field.attach(to: window.root)
        let reported = expectation(description: "handler error hook receives both failures")
        reported.expectedFulfillmentCount = 2
        app.onError { error, _ in
            XCTAssertEqual(error.code, "handler_failure")
            reported.fulfill()
        }
        let subscription = try await app.on(node: field.handle, event: "change") { _ in
            throw BridgeError(code: "handler_failure", message: "expected test failure")
        }
        await fulfillment(of: [reported], timeout: 4)
        await subscription.close()
        await app.close()
    }

    func testUnsubscribeDropsEventsBeforeHostAcknowledgement() async throws {
        #if os(Windows)
        let fixture = "../../target/debug/fixture-host.exe"
        #else
        let fixture = "../../target/debug/fixture-host"
        #endif
        let host = URL(fileURLWithPath: ProcessInfo.processInfo.environment["GRAFIKR_FIXTURE_HOST"] ?? fixture, relativeTo: URL(fileURLWithPath: FileManager.default.currentDirectoryPath)).standardizedFileURL
        var environment = ProcessInfo.processInfo.environment
        environment["GRAFIKR_FIXTURE_UNSUBSCRIBE_EVENT"] = "1"
        environment["GRAFIKR_FIXTURE_DELAY_UNSUBSCRIBE_RESPONSE_MS"] = "100"
        environment.removeValue(forKey: "GRAFIKR_FIXTURE_EARLY_SUBSCRIBE")
        let app = try App(host: host, environment: environment)
        try await app.start()
        let window = try await app.createWindow(title: "unsubscribe", width: 320, height: 240)
        let button = try await app.createNode(window: window, widget: "Button", properties: ["label":"unsubscribe"])
        try await button.attach(to: window.root)
        let received = expectation(description: "stale callback")
        received.isInverted = true
        let subscription = try await app.on(node: button.handle, event: "click") { _ in received.fulfill() }
        await subscription.close()
        await fulfillment(of: [received], timeout: 0.3)
        await app.close()
    }

    func testBlockedCallbackQueueEnforcesMessageAndByteLimits() async throws {
        try await assertCallbackOverflow(flag: "GRAFIKR_FIXTURE_EVENT_COUNT_OVERFLOW", event: "select")
        try await assertCallbackOverflow(flag: "GRAFIKR_FIXTURE_EVENT_BYTES_OVERFLOW", event: "resize")
    }

    private func assertCallbackOverflow(flag: String, event: String) async throws {
        #if os(Windows)
        let fixture = "../../target/debug/fixture-host.exe"
        #else
        let fixture = "../../target/debug/fixture-host"
        #endif
        let host = URL(fileURLWithPath: ProcessInfo.processInfo.environment["GRAFIKR_FIXTURE_HOST"] ?? fixture, relativeTo: URL(fileURLWithPath: FileManager.default.currentDirectoryPath)).standardizedFileURL
        var environment = ProcessInfo.processInfo.environment
        environment[flag] = "1"
        let app = try App(host: host, environment: environment)
        try await app.start()
        let window = try await app.createWindow(title: "bounded callbacks", width: 320, height: 240)
        let widget = event == "resize" ? "Canvas" : "VirtualList"
        let properties: [String: Any] = event == "resize"
            ? ["commands":[],"label":"overflow","width":120,"height":80]
            : ["row_count":10,"row_height":24,"width":200,"height":120]
        let node = try await app.createNode(window: window, widget: widget, properties: properties)

        let handlerStarted = expectation(description: "callback starts")
        let releaseHandler = DispatchSemaphore(value: 0)
        let subscription = try await app.on(node: node.handle, event: event) { _ in
            handlerStarted.fulfill()
            _ = releaseHandler.wait(timeout: .now() + 15)
        }
        await fulfillment(of: [handlerStarted], timeout: 10)
        try await Task.sleep(for: .seconds(1))
        do {
            _ = try await app.request("state.get", ["state":"0"])
            XCTFail("event queue overflow left the session open")
        } catch let error as BridgeError {
            if error.code != "queue_full" {
                throw BridgeError(code:error.code,message:"\(event) queue overflow did not close app")
            }
        }
        releaseHandler.signal()
        await subscription.close()
        await app.close()
    }

    func testCollectionProviderServesVisibleRange() async throws {
        #if os(Windows)
        let fixture = "../../target/debug/fixture-host.exe"
        #else
        let fixture = "../../target/debug/fixture-host"
        #endif
        let host = URL(fileURLWithPath: ProcessInfo.processInfo.environment["GRAFIKR_FIXTURE_HOST"] ?? fixture, relativeTo: URL(fileURLWithPath: FileManager.default.currentDirectoryPath)).standardizedFileURL
        let app = try App(host: host)
        try await app.start()
        let window = try await app.createWindow(title: "collection", width: 320, height: 240)
        let list = try await app.createNode(window: window, widget: "VirtualList", properties: ["row_count":1,"row_height":24,"height":120])
        try await list.attach(to: window.root)
        let provider = try await app.createCollectionProvider(node:list,metadata:CollectionMetadata(rowCount:1,rowHeight:24,columns:["label"]))
        let request = try await provider.nextRequest()
        XCTAssertNotNil(request)
        XCTAssertEqual(request?.revision,"1")
        XCTAssertEqual(request?.start,0)
        if let request { try await provider.provide(request,rows:[["row"]]) }
        try await provider.update(CollectionMetadata(rowCount:1,rowHeight:24,columns:["label"]))
        await provider.close()
        await app.close()
    }
}
