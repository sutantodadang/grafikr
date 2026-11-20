import Foundation

public struct BridgeError: Error, Sendable { public let code: String; public let message: String }
public struct Event: Sendable { public let sequence: String; public let name: String; public let source: String; public let payload: Data }

public final class App: @unchecked Sendable {
    public static let maxFrame = 8 * 1024 * 1024
    public static let maxOutstandingRequests = 256
    private static let maxQueuedCallbacks = 1024
    private static let maxQueuedCallbackBytes = 16 * 1024 * 1024
    private let process: Process
    private let input: Pipe
    private let output: Pipe
    private let readerQueue = DispatchQueue(label: "grafikr.reader")
    private let callbackQueue = DispatchQueue(label: "grafikr.callbacks")
    private let lock = NSLock()
    private var nextID: UInt64 = 0
    private var pending: [String: CheckedContinuation<Data, Error>] = [:]
    private var handlers: [String: [UUID: @Sendable (Event) throws -> Void]] = [:]
    private var errorHandler: (@Sendable (BridgeError, Event) -> Void)?
    private var queuedCallbacks = 0
    private var queuedCallbackBytes = 0
    private var frame = Data()
    private var closed = false

    public convenience init(host: URL) throws {
        try self.init(host: host, environment: ProcessInfo.processInfo.environment)
    }

    init(host: URL, environment: [String: String]) throws {
        process = Process(); input = Pipe(); output = Pipe(); let errors = Pipe()
        process.executableURL = host; process.arguments = []; process.environment = environment; process.standardInput = input; process.standardOutput = output; process.standardError = errors
        try process.run()
        errors.fileHandleForReading.readabilityHandler = { handle in if handle.availableData.isEmpty { handle.readabilityHandler = nil } }
        let reader = output.fileHandleForReading
        readerQueue.async { [weak self] in
            while true {
                let chunk = reader.availableData
                guard !chunk.isEmpty else { break }
                self?.consume(chunk)
            }
            self?.endOfOutput()
        }
    }

    public func start() async throws {
        let result = try await request("hello", ["protocol_major":1,"protocol_minor":0,"sdk_version":"1.1.0"])
        let value = try JSONSerialization.jsonObject(with: result) as! [String: Any]
        guard (value["protocol"] as? [String: Int])?["major"] == 1 else { throw BridgeError(code:"version_mismatch",message:"Protocol 1 required") }
    }

    public func request(_ op: String, _ args: [String: Any]) async throws -> Data {
        let id: String = try synchronized { guard !closed else { throw BridgeError(code:"connection_closed",message:"App is closed") }; nextID += 1; return String(nextID) }
        let object: [String: Any] = ["id":id,"op":op,"args":args]
        var bytes = try JSONSerialization.data(withJSONObject: object, options: [.fragmentsAllowed]); bytes.append(10)
        guard bytes.count <= Self.maxFrame else { throw BridgeError(code:"frame_too_large",message:"Request exceeds 8 MiB") }
        return try await withCheckedThrowingContinuation { continuation in
            let registrationError: BridgeError? = synchronized {
                if closed { return BridgeError(code:"connection_closed",message:"App is closed") }
                if pending.count >= Self.maxOutstandingRequests { return BridgeError(code:"queue_full",message:"Outstanding request count quota reached") }
                pending[id] = continuation
                return nil
            }
            if let registrationError { continuation.resume(throwing:registrationError); return }
            do { try input.fileHandleForWriting.write(contentsOf: bytes) }
            catch {
                let removed = synchronized { pending.removeValue(forKey:id) != nil }
                if removed { continuation.resume(throwing:error) }
            }
        }
    }

    public func createWindow(title: String, width: Double, height: Double) async throws -> Window {
        let data = try await request("window.create", ["title":title,"width":width,"height":height])
        let result = try JSONSerialization.jsonObject(with:data) as! [String:String]
        return Window(app:self,handle:result["window"]!,root:result["root"]!)
    }
    public func createNode(window: Window, widget: String, properties: [String:Any]) async throws -> Node {
        let data = try await request("node.create", ["window":window.handle,"widget":widget,"properties":properties])
        let result = try JSONSerialization.jsonObject(with:data) as! [String:String]
        return Node(app:self,handle:result["node"]!)
    }
    public func createState(type: String, value: Any) async throws -> State {
        let data = try await request("state.create", ["value":["type":type,"value":value]])
        let result = try JSONSerialization.jsonObject(with:data) as! [String:String]
        return State(app:self,handle:result["state"]!)
    }
    public func createCollectionProvider(node: Node, metadata: CollectionMetadata) async throws -> CollectionProvider {
        try await CollectionProvider.create(node:node,metadata:metadata)
    }

    public func onError(_ handler: @escaping @Sendable (BridgeError, Event) -> Void) {
        synchronized { errorHandler = handler }
    }

    @discardableResult public func on(node: String, event: String, handler: @escaping @Sendable (Event) throws -> Void) async throws -> Subscription {
        let id = UUID(); let key = node + "\0" + event
        synchronized { handlers[key,default:[:]][id] = handler }
        let reply: [String: String]
        do {
            let data = try await request("event.subscribe", ["node":node,"event":event])
            reply = try JSONSerialization.jsonObject(with:data) as! [String: String]
        } catch {
            _ = synchronized { handlers[key]?.removeValue(forKey:id) }
            throw error
        }
        return Subscription { [weak self] in
            guard let self else { return }
            _ = self.synchronized { self.handlers[key]?.removeValue(forKey:id) }
            _ = try? await self.request("event.unsubscribe", ["subscription":reply["subscription"] ?? ""])
        }
    }

    public func close() async {
        _ = try? await request("shutdown", [:]); synchronized { failLocked(BridgeError(code:"connection_closed",message:"App is closed")) }
        try? input.fileHandleForWriting.close(); if process.isRunning { process.terminate() }
        if process.isRunning { try? await Task.sleep(for:.seconds(5)); if process.isRunning { process.interrupt() } }
    }

    private func consume(_ data: Data) {
        guard !data.isEmpty else { return }; lock.lock()
        frame.append(data)
        if frame.count > Self.maxFrame && !frame.contains(10) { failLocked(BridgeError(code:"frame_too_large",message:"Host frame exceeds 8 MiB")); lock.unlock(); return }
        while let lf = frame.firstIndex(of:10) {
            if frame.distance(from: frame.startIndex, to: lf) + 1 > Self.maxFrame { failLocked(BridgeError(code:"frame_too_large",message:"Host frame exceeds 8 MiB")); break }
            let line = frame[..<lf]; frame.removeSubrange(...lf)
            guard let m = try? JSONSerialization.jsonObject(with:Data(line)) as? [String:Any], let kind=m["kind"] as? String else { failLocked(BridgeError(code:"invalid_frame",message:"Invalid host JSON")); break }
            if kind == "response", let id=m["id"] as? String, let continuation=pending.removeValue(forKey:id) {
                if m["ok"] as? Bool == true { continuation.resume(returning:(try? JSONSerialization.data(withJSONObject:m["result"] ?? [:])) ?? Data("{}".utf8)) }
                else { let e=m["error"] as? [String:Any] ?? [:]; continuation.resume(throwing:BridgeError(code:e["code"] as? String ?? "internal_failure",message:e["message"] as? String ?? "Request failed")) }
            } else if kind == "event" {
                let event=Event(sequence:m["sequence"] as? String ?? "0",name:m["event"] as? String ?? "",source:m["source"] as? String ?? "0",payload:(try? JSONSerialization.data(withJSONObject:m["payload"] ?? [:])) ?? Data())
                let key=event.source+"\0"+event.name
                if handlers[key]?.isEmpty == false {
                    let bytes=event.payload.count
                    guard queuedCallbacks < Self.maxQueuedCallbacks,
                          queuedCallbackBytes + bytes <= Self.maxQueuedCallbackBytes else {
                        failLocked(BridgeError(code:"queue_full",message:"Callback queue quota reached"))
                        lock.unlock()
                        if process.isRunning { process.terminate() }
                        return
                    }
                    queuedCallbacks += 1
                    queuedCallbackBytes += bytes
                    callbackQueue.async { [weak self] in
                        guard let self else { return }
                        let callbacks: [@Sendable (Event) throws -> Void] = self.synchronized {
                            guard !self.closed else { return [] }
                            return self.handlers[key].map { Array($0.values) } ?? []
                        }
                        for callback in callbacks {
                            do { try callback(event) }
                            catch {
                                let reported = error as? BridgeError ?? BridgeError(code:"callback_failure",message:"Event handler failed")
                                self.synchronized { self.errorHandler }?(reported,event)
                            }
                        }
                        self.synchronized {
                            self.queuedCallbacks -= 1
                            self.queuedCallbackBytes -= bytes
                        }
                    }
                }
            }
        }
        lock.unlock()
    }
    private func endOfOutput(_ cause: Error? = nil) {
        lock.lock()
        if !closed {
            let error = BridgeError(
                code: frame.isEmpty ? "connection_closed" : "invalid_frame",
                message: cause == nil ? "Host connection closed" : "Host output could not be read"
            )
            failLocked(error)
        }
        lock.unlock()
    }
    private func failLocked(_ error: Error) { closed=true; pending.values.forEach{$0.resume(throwing:error)}; pending.removeAll() }
    private func synchronized<T>(_ body: () throws -> T) rethrows -> T { lock.lock(); defer { lock.unlock() }; return try body() }
}

public final class Subscription: @unchecked Sendable { private let disposeBody: @Sendable () async -> Void; public init(_ body: @escaping @Sendable () async -> Void){disposeBody=body}; public func close() async { await disposeBody() } }
public struct Window: Sendable { public let app: App; public let handle: String; public let root: String; public func close() async throws { _ = try await app.request("window.close",["window":handle]) } }
public struct Node: Sendable { public let app: App; public let handle: String; public func attach(to parent: String) async throws { _ = try await app.request("node.attach",["node":handle,"parent":parent]) }; public func update(_ properties: [String:Any]) async throws { _ = try await app.request("property.update",["node":handle,"properties":properties]) } }
public struct State: Sendable { public let app: App; public let handle: String; public func get() async throws -> Data { try await app.request("state.get",["state":handle]) }; public func set(type: String,value: Any) async throws { _ = try await app.request("state.set",["state":handle,"value":["type":type,"value":value]]) }; public func destroy() async throws { _ = try await app.request("state.destroy",["state":handle]) } }

public struct CollectionMetadata: Sendable {
    public let rowCount: Int
    public let rowHeight: Double
    public let columns: [String]
    public init(rowCount: Int, rowHeight: Double, columns: [String]) { self.rowCount=rowCount; self.rowHeight=rowHeight; self.columns=columns }
    fileprivate func wire() throws -> [String:Any] {
        guard rowCount >= 0, rowHeight.isFinite, rowHeight > 0, !columns.isEmpty else { throw BridgeError(code:"invalid_value",message:"Collection metadata is invalid") }
        return ["row_count":rowCount,"row_height":rowHeight,"columns":columns]
    }
}

public struct CollectionRequest: Sendable {
    public let revision: String
    public let request: String
    public let start: Int
    public let count: Int
    fileprivate struct Payload: Decodable { let revision: String; let request: String; let start: Int; let count: Int }
    fileprivate static func decode(_ event: Event) throws -> CollectionRequest {
        let payload = try JSONDecoder().decode(Payload.self, from:event.payload)
        guard payload.start >= 0, (0...512).contains(payload.count) else {
            throw BridgeError(code:"invalid_frame",message:"Collection request fields are invalid")
        }
        return CollectionRequest(revision:payload.revision,request:payload.request,start:payload.start,count:payload.count)
    }
}

public final class CollectionProvider: @unchecked Sendable {
    private let node: Node
    private let lock = NSLock()
    private var requests: [CollectionRequest] = []
    private var waiter: (id: UUID, continuation: CheckedContinuation<CollectionRequest?, Error>)?
    private var failure: BridgeError?
    private var subscription: Subscription?
    private var closed = false
    private var disposed = false

    private init(node: Node) { self.node=node }

    fileprivate static func create(node: Node, metadata: CollectionMetadata) async throws -> CollectionProvider {
        let provider=CollectionProvider(node:node)
        provider.subscription=try await node.app.on(node:node.handle,event:"data.request") { event in provider.receive(event) }
        do {
            var args=try metadata.wire(); args["node"]=node.handle
            _ = try await node.app.request("collection.create",args)
            return provider
        } catch { await provider.close(); throw error }
    }

    public func nextRequest(timeout: TimeInterval = 30) async throws -> CollectionRequest? {
        try await withCheckedThrowingContinuation { continuation in
            let waiterID=UUID()
            let state: (Result<CollectionRequest?,Error>?,Bool) = synchronized {
                let nanoseconds=timeout * 1_000_000_000
                guard timeout.isFinite, timeout >= 0, nanoseconds.isFinite, nanoseconds < Double(UInt64.max) else {
                    return (.failure(BridgeError(code:"invalid_value",message:"Collection timeout is invalid")),false)
                }
                if !requests.isEmpty { return (.success(requests.removeFirst()),false) }
                if let failure { self.failure=nil; return (.failure(failure),false) }
                if closed { return (.success(nil),false) }
                if waiter != nil { return (.failure(BridgeError(code:"invalid_lifecycle",message:"Only one collection reader is allowed")),false) }
                guard timeout > 0 else { return (.success(nil),false) }
                waiter=(waiterID,continuation)
                return (nil,true)
            }
            if let immediate=state.0 { continuation.resume(with:immediate) }
            else if state.1 {
                let nanoseconds=UInt64(timeout * 1_000_000_000)
                Task { [weak self] in
                    try? await Task.sleep(nanoseconds:nanoseconds)
                    self?.timeoutWaiter(waiterID)
                }
            }
        }
    }

    public func provide(_ request: CollectionRequest, rows: [[Any]]) async throws {
        try ensureOpen()
        _ = try await node.app.request("data.provide",["node":node.handle,"revision":request.revision,"request":request.request,"start":request.start,"rows":rows])
    }

    public func update(_ metadata: CollectionMetadata) async throws {
        try ensureOpen()
        _ = try await node.app.request("collection.update",["node":node.handle,"metadata":try metadata.wire()])
    }

    public func close() async {
        let state = synchronized { () -> (Bool, CheckedContinuation<CollectionRequest?,Error>?, Subscription?) in
            guard !disposed else { return (false,nil,nil) }
            disposed=true; closed=true; requests.removeAll(); failure=nil
            let pending=waiter?.continuation; waiter=nil
            let subscription=self.subscription; self.subscription=nil
            return (true,pending,subscription)
        }
        guard state.0 else { return }
        state.1?.resume(returning:nil)
        await state.2?.close()
    }

    private func receive(_ event: Event) {
        let result=Result { try CollectionRequest.decode(event) }
        lock.lock()
        guard !closed else { lock.unlock(); return }
        if case .failure(let error)=result {
            let bridgeError=error as? BridgeError ?? BridgeError(code:"invalid_frame",message:"Collection request is invalid")
            failure=bridgeError
            let pending=waiter?.continuation; waiter=nil; lock.unlock()
            pending?.resume(throwing:bridgeError)
            return
        }
        guard case .success(let request)=result else { lock.unlock(); return }
        if let pending=waiter?.continuation { waiter=nil; lock.unlock(); pending.resume(returning:request) }
        else if requests.isEmpty { requests.append(request); lock.unlock() }
        else { failure=BridgeError(code:"queue_full",message:"Collection provider request queue is full"); lock.unlock() }
    }

    private func ensureOpen() throws {
        try synchronized { if disposed { throw BridgeError(code:"connection_closed",message:"Collection provider is closed") } }
    }

    private func timeoutWaiter(_ id: UUID) {
        let pending=synchronized { () -> CheckedContinuation<CollectionRequest?,Error>? in
            guard waiter?.id == id else { return nil }
            let pending=waiter?.continuation; waiter=nil
            return pending
        }
        pending?.resume(returning:nil)
    }

    private func synchronized<T>(_ body: () throws -> T) rethrows -> T { lock.lock(); defer { lock.unlock() }; return try body() }
}
