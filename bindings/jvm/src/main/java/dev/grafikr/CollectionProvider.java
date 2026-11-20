package dev.grafikr;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.util.concurrent.ArrayBlockingQueue;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.concurrent.TimeUnit;

public final class CollectionProvider implements AutoCloseable {
    private static final Object CLOSED = new Object();
    private final Node node;
    private final ArrayBlockingQueue<Object> requests = new ArrayBlockingQueue<>(1);
    private volatile boolean closed;
    private volatile BridgeException failure;
    private final AtomicBoolean disposed = new AtomicBoolean();
    private Subscription subscription;

    CollectionProvider(Node node) { this.node = node; }

    public static CollectionProvider create(Node node, long rowCount, double rowHeight, String... columns) throws Exception {
        CollectionProvider provider = new CollectionProvider(node);
        try {
            provider.subscription = node.app().on(node.handle(), "data.request", provider::receive);
            JsonObject args = metadata(rowCount, rowHeight, columns);
            args.addProperty("node", node.handle());
            node.app().request("collection.create", args);
            return provider;
        } catch (Exception error) {
            provider.close();
            throw error;
        }
    }

    public CollectionRequest nextRequest(long timeout, TimeUnit unit) throws InterruptedException {
        Object value = requests.poll(timeout, unit);
        if (value == CLOSED) {
            BridgeException error = failure;
            failure = null;
            if (error != null) throw error;
            return null;
        }
        return value == null ? null : (CollectionRequest) value;
    }

    public void provide(CollectionRequest request, JsonArray rows) throws Exception {
        if (closed) throw new BridgeException("connection_closed", "Collection provider is closed");
        JsonObject args = new JsonObject();
        args.addProperty("node", node.handle());
        args.addProperty("revision", request.revision());
        args.addProperty("request", request.request());
        args.addProperty("start", request.start());
        args.add("rows", rows);
        node.app().request("data.provide", args);
    }

    public void update(long rowCount, double rowHeight, String... columns) throws Exception {
        if (closed) throw new BridgeException("connection_closed", "Collection provider is closed");
        JsonObject args = new JsonObject();
        args.addProperty("node", node.handle());
        args.add("metadata", metadata(rowCount, rowHeight, columns));
        node.app().request("collection.update", args);
    }

    @Override public synchronized void close() throws Exception {
        if (!disposed.compareAndSet(false, true)) return;
        closed = true;
        requests.clear();
        requests.offer(CLOSED);
        if (subscription != null) subscription.close();
    }

    synchronized void receive(JsonObject event) {
        if (closed) return;
        if (!requests.offer(CollectionRequest.from(event)))
        {
            failure = new BridgeException("queue_full", "Collection provider request queue is full");
            closed = true;
            requests.clear();
            requests.offer(CLOSED);
        }
    }

    private static JsonObject metadata(long rowCount, double rowHeight, String[] columns) throws BridgeException {
        if (rowCount < 0 || !Double.isFinite(rowHeight) || rowHeight <= 0)
            throw new BridgeException("invalid_value", "Collection metadata is invalid");
        JsonArray names = new JsonArray();
        for (String column : columns) names.add(column);
        JsonObject metadata = new JsonObject();
        metadata.addProperty("row_count", rowCount);
        metadata.addProperty("row_height", rowHeight);
        metadata.add("columns", names);
        return metadata;
    }
}
