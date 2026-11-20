package dev.grafikr;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.google.gson.JsonObject;
import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonParser;
import java.util.Base64;
import java.util.Map;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.Duration;
import java.lang.reflect.Field;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.Semaphore;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.atomic.AtomicInteger;
import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assumptions.assumeTrue;

final class ContractTest {
    @Test
    void createsEverySharedWidgetAndReceivesItsDeclaredEvents() throws Exception {
        assumeTrue("1".equals(System.getenv("GRAFIKR_FIXTURE_WIDGET_INVENTORY")));
        try (App app = new App(fixtureHost(repositoryRoot()))) {
            Window window = app.createWindow("JVM widget inventory", 640, 480);
            State state = app.createState(i64(1));
            String image = uploadImage(app);
            JsonObject widgets = JsonParser.parseString(Files.readString(
                repositoryRoot().resolve("bindings/protocol/v1/widgets.json")))
                .getAsJsonObject().getAsJsonObject("widgets");
            for (Map.Entry<String, JsonElement> entry : widgets.entrySet()) {
                String kind = entry.getKey();
                JsonObject spec = entry.getValue().getAsJsonObject();
                JsonObject properties = new JsonObject();
                for (Map.Entry<String, JsonElement> property : spec.getAsJsonObject("properties").entrySet()) {
                    properties.add(property.getKey(), sample(kind, property.getKey(), property.getValue().getAsString(), state.handle(), image));
                }
                Node node = app.createNode(window, kind, properties);
                node.attach(window.root());
                JsonArray events = spec.has("events") ? spec.getAsJsonArray("events") : new JsonArray();
                for (JsonElement eventValue : events) {
                    String event = eventValue.getAsString();
                    CountDownLatch received = new CountDownLatch(1);
                    try (Subscription ignored = app.on(node.handle(), event, message -> {
                        if (node.handle().equals(message.get("source").getAsString()) && event.equals(message.get("event").getAsString()))
                            received.countDown();
                    })) {
                        if (event.equals("data.request")) {
                            JsonObject collection = new JsonObject(); collection.addProperty("node", node.handle());
                            collection.addProperty("row_count", 1); collection.addProperty("row_height", 24);
                            JsonArray columns = new JsonArray(); columns.add("label"); collection.add("columns", columns);
                            app.request("collection.create", collection);
                        }
                        assertTrue(received.await(2, TimeUnit.SECONDS), kind + "@" + event + " was not delivered");
                    }
                }
                node.destroy();
            }
            JsonObject destroyImage = new JsonObject(); destroyImage.addProperty("image", image);
            app.request("image.destroy", destroyImage);
            state.close();
            window.close();
        }
    }

    @Test
    void nestedCallbackRequestCompletesAndHandlerErrorsReachHook() throws Exception {
        Path root = repositoryRoot();
        CountDownLatch handled = new CountDownLatch(1);
        try (App app = new App(fixtureHost(root))) {
            app.onError(error -> handled.countDown());
            Window window = app.createWindow("callback contract", 320, 240);
            Node button = app.createNode(window, "Button", new JsonObject());
            button.attach(window.root());
            try (Subscription ignored = app.on(button.handle(), "click", event -> {
                try {
                    JsonObject value = i64(7);
                    JsonObject args = new JsonObject();
                    args.add("value", value);
                    app.request("state.create", args);
                } catch (Exception error) {
                    throw new RuntimeException(error);
                }
                throw new IllegalStateException("callback failure probe");
            })) {
                assertTrue(handled.await(2, TimeUnit.SECONDS));
            }
            window.close();
        }
    }

    @Test
    void unsubscribeSuppressesAnEventSentBeforeItsResponse() throws Exception {
        assumeTrue("1".equals(System.getenv("GRAFIKR_FIXTURE_UNSUBSCRIBE_EVENT")));
        Path root = repositoryRoot();
        AtomicInteger calls = new AtomicInteger();
        CountDownLatch earlyEvent = new CountDownLatch(1);
        try (App app = new App(fixtureHost(root))) {
            Window window = app.createWindow("unsubscribe contract", 320, 240);
            Node button = app.createNode(window, "Button", new JsonObject());
            button.attach(window.root());
            Subscription subscription = app.on(button.handle(), "click", event -> {
                calls.incrementAndGet();
                earlyEvent.countDown();
            });
            assertTrue(earlyEvent.await(2, TimeUnit.SECONDS));
            subscription.close();
            Thread.sleep(250);
            assertEquals(1, calls.get(), "unsubscribe-delivered stale event reached the handler");
            window.close();
        }
    }

    @Test
    void subscriptionReceivesAnEventBeforeItsResponse() throws Exception {
        Path root = Path.of("").toAbsolutePath();
        while (root != null && !Files.exists(root.resolve("Cargo.toml"))) root = root.getParent();
        if (root == null) throw new IllegalStateException("repository root not found");
        Path host = root.resolve("target/debug/fixture-host" + (System.getProperty("os.name").startsWith("Windows") ? ".exe" : ""));
        CountDownLatch received = new CountDownLatch(1);
        try (App app = new App(host)) {
            Window window = app.createWindow("fixture", 320, 240);
            Node button = app.createNode(window, "Button", new JsonObject());
            button.attach(window.root());
            try (Subscription ignored = app.on(button.handle(), "click", event -> {
                if (button.handle().equals(event.get("source").getAsString())) received.countDown();
            })) {
                assertTrue(received.await(Duration.ofSeconds(2).toMillis(), TimeUnit.MILLISECONDS));
            }
            window.close();
        }
    }

    @Test
    void stateRoundTripsBothSigned64BitBoundaries() throws Exception {
        Path root = Path.of("").toAbsolutePath();
        while (root != null && !Files.exists(root.resolve("Cargo.toml"))) root = root.getParent();
        if (root == null) throw new IllegalStateException("repository root not found");
        Path host = root.resolve("target/debug/fixture-host" + (System.getProperty("os.name").startsWith("Windows") ? ".exe" : ""));
        try (App app = new App(host)) {
            State state = app.createState(i64(Long.MIN_VALUE));
            assertEquals(Long.toString(Long.MIN_VALUE), state.get().get("value").getAsString());
            state.set(i64(Long.MAX_VALUE));
            assertEquals(Long.toString(Long.MAX_VALUE), state.get().get("value").getAsString());
            state.close();
            JsonObject text = new JsonObject(); text.addProperty("type", "string");
            String large = "line one\nline two 雪 😀" + "x".repeat(100 * 1024);
            text.addProperty("value", large);
            State largeState = app.createState(text);
            assertEquals(large, largeState.get().get("value").getAsString());
            largeState.close();
        }
    }

    @Test
    void correlatesConcurrentOutOfOrderResponses() throws Exception {
        assumeTrue("1".equals(System.getenv("GRAFIKR_FIXTURE_REVERSE_RESPONSES")));
        try (App app = new App(fixtureHost(repositoryRoot()))) {
            State first = app.createState(stringValue("first"));
            State second = app.createState(stringValue("second"));
            CountDownLatch start = new CountDownLatch(1);
            ExecutorService workers = Executors.newFixedThreadPool(2);
            try {
                Future<JsonObject> firstReply = workers.submit(() -> { start.await(); return first.get(); });
                Future<JsonObject> secondReply = workers.submit(() -> { start.await(); return second.get(); });
                start.countDown();
                assertEquals("first", firstReply.get(2, TimeUnit.SECONDS).get("value").getAsString());
                assertEquals("second", secondReply.get(2, TimeUnit.SECONDS).get("value").getAsString());
            } finally {
                workers.shutdownNow();
                first.close(); second.close();
            }
        }
    }

    @Test
    void rejectsNonFiniteStateBeforeSending() throws Exception {
        try (App app = new App(fixtureHost(repositoryRoot()))) {
            JsonObject value = new JsonObject(); value.addProperty("type", "f64"); value.addProperty("value", Double.NaN);
            BridgeException error = assertThrows(BridgeException.class, () -> app.createState(value));
            assertEquals("invalid_value", error.code());
        }
    }

    @Test
    void eofCompletesAll256OutstandingRequests() throws Exception {
        assumeTrue("1".equals(System.getenv("GRAFIKR_FIXTURE_HOLD_RESPONSES")) &&
            System.getenv("GRAFIKR_FIXTURE_EXIT_AFTER_HELLO_MS") != null);
        try (App app = new App(fixtureHost(repositoryRoot()))) {
            ExecutorService workers = Executors.newFixedThreadPool(256);
            CountDownLatch start = new CountDownLatch(1);
            CountDownLatch attempted = new CountDownLatch(256);
            List<Future<String>> replies = new ArrayList<>();
            try {
                for (int i = 0; i < 256; i++) replies.add(workers.submit(() -> {
                    attempted.countDown(); start.await();
                    JsonObject args = new JsonObject(); args.addProperty("state", "0");
                    try { app.request("state.get", args); return "unexpected_success"; }
                    catch (BridgeException error) { return error.code(); }
                    catch (Exception error) { throw new RuntimeException(error); }
                }));
                assertTrue(attempted.await(2, TimeUnit.SECONDS));
                start.countDown();
                Field slotsField = App.class.getDeclaredField("requestSlots"); slotsField.setAccessible(true);
                Semaphore slots = (Semaphore) slotsField.get(app);
                long deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(2);
                while (slots.availablePermits() != 0 && System.nanoTime() < deadline) Thread.sleep(5);
                assertEquals(0, slots.availablePermits(), "all 256 requests should be pending before EOF");
                for (Future<String> reply : replies) assertEquals("connection_closed", reply.get(5, TimeUnit.SECONDS));
            } finally {
                start.countDown(); workers.shutdownNow();
            }
        }
    }

    @Test
    void blockedEventHandlerCannotExhaustReaderWithoutClosingSession() throws Exception {
        assumeTrue("1".equals(System.getenv("GRAFIKR_FIXTURE_EVENT_BYTES_OVERFLOW")));
        try (App app = new App(fixtureHost(repositoryRoot()))) {
            Window window = app.createWindow("queue limits", 320, 240);
            JsonObject properties = new JsonObject(); properties.addProperty("label", "canvas");
            properties.addProperty("width", 100); properties.addProperty("height", 100);
            properties.add("commands", new JsonArray());
            Node canvas = app.createNode(window, "Canvas", properties); canvas.attach(window.root());
            CountDownLatch entered = new CountDownLatch(1);
            CountDownLatch release = new CountDownLatch(1);
            try {
                app.on(canvas.handle(), "resize", event -> {
                entered.countDown();
                try { release.await(); } catch (InterruptedException error) { Thread.currentThread().interrupt(); }
                });
                assertTrue(entered.await(2, TimeUnit.SECONDS));
                Field closedField = App.class.getDeclaredField("closed"); closedField.setAccessible(true);
                long deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(3);
                while (!closedField.getBoolean(app) && System.nanoTime() < deadline) Thread.sleep(5);
                assertTrue(closedField.getBoolean(app), "callback byte quota did not stop the overflowing session");
            } finally { release.countDown(); }
        }
    }

    @Test
    void requestQueueRejectsFramesAboveSixteenMiB() throws Exception {
        assumeTrue("1".equals(System.getenv("GRAFIKR_FIXTURE_HOLD_ALL_RESPONSES")));
        try (App app = new App(fixtureHost(repositoryRoot()))) {
            ExecutorService workers = Executors.newFixedThreadPool(3);
            CountDownLatch start = new CountDownLatch(1);
            String padding = "x".repeat(6 * 1024 * 1024);
            List<Future<String>> accepted = new ArrayList<>();
            try {
                for (int i = 0; i < 2; i++) accepted.add(workers.submit(() -> {
                    start.await();
                    JsonObject value = stringValue(padding);
                    JsonObject args = new JsonObject(); args.addProperty("state", "0"); args.add("value", value);
                    try { app.request("state.set", args); return "unexpected_success"; }
                    catch (BridgeException error) { return error.code(); }
                }));
                start.countDown();
                Field pendingField = App.class.getDeclaredField("pending"); pendingField.setAccessible(true);
                @SuppressWarnings("unchecked") var pending = (java.util.Map<String, ?>) pendingField.get(app);
                long deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(5);
                while (pending.size() < 2 && System.nanoTime() < deadline) Thread.sleep(5);
                assertEquals(2, pending.size(), "both requests should be outstanding before testing the byte cap");
                Field bytesField = App.class.getDeclaredField("pendingBytes"); bytesField.setAccessible(true);
                assertTrue(((java.util.concurrent.atomic.AtomicLong) bytesField.get(app)).get() > 12L * 1024 * 1024,
                    "pending request frames were not counted");
                Future<String> overflow = workers.submit(() -> {
                    JsonObject value = stringValue(padding);
                    JsonObject args = new JsonObject(); args.addProperty("state", "0"); args.add("value", value);
                    try { app.request("state.set", args); return "unexpected_success"; }
                    catch (BridgeException error) { return error.code(); }
                });
                assertEquals("queue_full", overflow.get(10, TimeUnit.SECONDS),
                    "pendingBytes=" + ((java.util.concurrent.atomic.AtomicLong) bytesField.get(app)).get());
                app.close();
                for (Future<String> request : accepted) assertEquals("connection_closed", request.get(3, TimeUnit.SECONDS));
            } finally { workers.shutdownNow(); }
        }
    }

    @Test
    void closeTimeoutLeavesUnrelatedSentinelAlive() throws Exception {
        assumeTrue(System.getenv("GRAFIKR_FIXTURE_DELAY_EXIT_MS") != null);
        boolean windows = System.getProperty("os.name").startsWith("Windows");
        Process sentinel = new ProcessBuilder(windows ? List.of("ping.exe", "-t", "127.0.0.1") : List.of("sleep", "30"))
            .redirectOutput(ProcessBuilder.Redirect.DISCARD).redirectError(ProcessBuilder.Redirect.DISCARD).start();
        try (App app = new App(fixtureHost(repositoryRoot()))) { app.close(); }
        try {
            assertTrue(sentinel.isAlive(), "closing Grafikr terminated the unrelated process");
        } finally {
            sentinel.destroyForcibly();
            sentinel.waitFor(2, TimeUnit.SECONDS);
        }
    }

    @Test
    void rejectsThe257thOutstandingRequestBeforeWriting() throws Exception {
        Path root = Path.of("").toAbsolutePath();
        while (root != null && !Files.exists(root.resolve("Cargo.toml"))) root = root.getParent();
        if (root == null) throw new IllegalStateException("repository root not found");
        Path host = root.resolve("target/debug/fixture-host" + (System.getProperty("os.name").startsWith("Windows") ? ".exe" : ""));
        try (App app = new App(host)) {
            Field slotsField = App.class.getDeclaredField("requestSlots");
            slotsField.setAccessible(true);
            Semaphore slots = (Semaphore) slotsField.get(app);
            assertTrue(slots.tryAcquire(256));
            BridgeException error = assertThrows(BridgeException.class, () -> app.request("shutdown", new JsonObject()));
            assertEquals("queue_full", error.code());
            slots.release(256);
        }
    }

    @Test
    void collectionProviderBoundsRequestsAndCloseWakesReaders() throws Exception {
        CollectionProvider reader = new CollectionProvider(null);
        reader.receive(event("1", "1", 2, 3));
        assertEquals(2, reader.nextRequest(1, TimeUnit.SECONDS).start());
        reader.close();
        assertEquals(null, reader.nextRequest(1, TimeUnit.SECONDS));

        CollectionProvider provider = new CollectionProvider(null);
        provider.receive(event("1", "1", 2, 3));
        provider.receive(event("1", "2", 5, 1));
        BridgeException error = assertThrows(BridgeException.class, () -> provider.nextRequest(1, TimeUnit.SECONDS));
        assertEquals("queue_full", error.code());
        JsonObject fractional = event("1", "3", 2, 3);
        fractional.getAsJsonObject("payload").addProperty("start", 2.5);
        assertEquals("invalid_frame", assertThrows(BridgeException.class, () -> CollectionRequest.from(fractional)).code());
        provider.close();
        assertEquals(null, provider.nextRequest(1, TimeUnit.SECONDS));
    }

    private static JsonObject event(String revision, String request, int start, int count) {
        JsonObject event = new JsonObject();
        JsonObject payload = new JsonObject();
        payload.addProperty("revision", revision);
        payload.addProperty("request", request);
        payload.addProperty("start", start);
        payload.addProperty("count", count);
        event.add("payload", payload);
        return event;
    }

    private static JsonObject i64(long value) {
        JsonObject scalar = new JsonObject();
        scalar.addProperty("type", "i64");
        scalar.addProperty("value", Long.toString(value));
        return scalar;
    }

    private static JsonObject stringValue(String value) {
        JsonObject scalar = new JsonObject(); scalar.addProperty("type", "string"); scalar.addProperty("value", value); return scalar;
    }

    private static JsonElement sample(String kind, String name, String type, String state, String image) {
        if (kind.equals("Radio") && name.equals("group")) return new com.google.gson.JsonPrimitive(state);
        if (kind.equals("TreeView") && name.equals("selected")) return new com.google.gson.JsonPrimitive("root");
        if (kind.equals("Spinner") && name.equals("size")) return new com.google.gson.JsonPrimitive(16);
        if (kind.equals("Slider")) {
            return new com.google.gson.JsonPrimitive(switch (name) { case "min" -> 0; case "max" -> 10; case "width" -> 120; default -> 1; });
        }
        if ((kind.equals("VirtualList") || kind.equals("DataGrid")) && name.equals("row_count")) return new com.google.gson.JsonPrimitive(1);
        if ((kind.equals("VirtualList") || kind.equals("DataGrid")) && name.equals("row_height")) return new com.google.gson.JsonPrimitive(24);
        return switch (type) {
            case "string" -> new com.google.gson.JsonPrimitive("sample");
            case "boolean" -> new com.google.gson.JsonPrimitive(false);
            case "u16" -> new com.google.gson.JsonPrimitive(1);
            case "u64" -> new com.google.gson.JsonPrimitive(0);
            case "number", "f32" -> new com.google.gson.JsonPrimitive(1);
            case "color" -> new com.google.gson.JsonPrimitive("#336699");
            case "transform" -> JsonParser.parseString("{\"x\":0,\"y\":0,\"scale\":1}");
            case "primary|secondary|icon" -> new com.google.gson.JsonPrimitive("primary");
            case "state" -> new com.google.gson.JsonPrimitive(state);
            case "ImageHandle" -> new com.google.gson.JsonPrimitive(image);
            case "string[]" -> JsonParser.parseString("[\"One\",\"Two\"]");
            case "GridColumn[]" -> JsonParser.parseString("[{\"title\":\"Column\",\"width\":120}]");
            case "TreeNode[]" -> JsonParser.parseString("[{\"key\":\"root\",\"label\":\"Root\",\"children\":[]}]");
            case "CanvasCommand[]" -> JsonParser.parseString("[{\"kind\":\"rect\",\"rect\":{\"x\":0,\"y\":0,\"width\":1,\"height\":1},\"color\":\"#336699\",\"radius\":0}]");
            default -> throw new IllegalArgumentException("No JVM conformance sample for " + kind + "." + name + ": " + type);
        };
    }

    private static String uploadImage(App app) throws Exception {
        JsonObject begin = new JsonObject(); begin.addProperty("width", 1); begin.addProperty("height", 1); begin.addProperty("byte_length", 4);
        String upload = app.request("image.begin", begin).get("upload").getAsString();
        JsonObject chunk = new JsonObject(); chunk.addProperty("upload", upload); chunk.addProperty("offset", 0);
        chunk.addProperty("data", Base64.getEncoder().encodeToString(new byte[] {0, 0, 0, (byte) 255}));
        app.request("image.chunk", chunk);
        JsonObject finalize = new JsonObject(); finalize.addProperty("upload", upload);
        return app.request("image.finalize", finalize).get("image").getAsString();
    }

    private static Path repositoryRoot() {
        Path root = Path.of("").toAbsolutePath();
        while (root != null && !Files.exists(root.resolve("Cargo.toml"))) root = root.getParent();
        if (root == null) throw new IllegalStateException("repository root not found");
        return root;
    }

    private static Path fixtureHost(Path root) {
        return root.resolve("target/debug/fixture-host" + (System.getProperty("os.name").startsWith("Windows") ? ".exe" : ""));
    }
}
