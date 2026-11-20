package dev.grafikr;

import com.google.gson.*;
import java.io.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.UUID;
import java.util.concurrent.*;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.concurrent.atomic.AtomicLong;
import java.util.function.Consumer;

public final class App implements AutoCloseable {
    public static final int MAX_FRAME = 8 * 1024 * 1024;
    private static final int MAX_OUTSTANDING_REQUESTS = 256;
    private static final long MAX_PENDING_BYTES = 16L * 1024 * 1024;
    private final Process process;
    private final OutputStream input;
    private final ConcurrentMap<String,CompletableFuture<JsonElement>> pending = new ConcurrentHashMap<>();
    private final ConcurrentMap<String,Integer> pendingFrameBytes = new ConcurrentHashMap<>();
    private final ConcurrentMap<String,EventHandler> handlers = new ConcurrentHashMap<>();
    private final ThreadPoolExecutor callbacks = new ThreadPoolExecutor(1,1,0L,TimeUnit.MILLISECONDS,new ArrayBlockingQueue<>(1024));
    private final AtomicInteger callbackCount = new AtomicInteger();
    private final AtomicLong callbackBytes = new AtomicLong();
    private final AtomicLong pendingBytes = new AtomicLong();
    private final AtomicLong ids = new AtomicLong();
    private final Semaphore requestSlots = new Semaphore(MAX_OUTSTANDING_REQUESTS);
    private final Object writeLock = new Object();
    private volatile boolean closed;
    private volatile Consumer<Throwable> errorHandler = ignored -> {};

    private static final class EventHandler {
        final Consumer<JsonObject> callback;
        volatile boolean active = true;
        EventHandler(Consumer<JsonObject> callback) { this.callback = callback; }
    }

    public App(Path host) throws Exception {
        process = new ProcessBuilder(host.toAbsolutePath().toString()).redirectError(ProcessBuilder.Redirect.INHERIT).start();
        input = new BufferedOutputStream(process.getOutputStream());
        Thread reader = new Thread(() -> readLoop(process.getInputStream()), "grafikr-reader"); reader.setDaemon(true); reader.start();
        JsonObject args = new JsonObject(); args.addProperty("protocol_major",1); args.addProperty("protocol_minor",0); args.addProperty("sdk_version","1.1.0");
        JsonObject hello = request("hello",args);
        if (hello.getAsJsonObject("protocol").get("major").getAsInt()!=1) { close(); throw new BridgeException("version_mismatch","Protocol 1 required"); }
    }
    public JsonObject request(String op, JsonObject args) throws Exception { return request(op,args,30); }
    public void onError(Consumer<Throwable> handler) { errorHandler = java.util.Objects.requireNonNull(handler); }
    private JsonObject request(String op, JsonObject args, int timeoutSeconds) throws Exception {
        if(closed) throw new BridgeException("connection_closed","App is closed");
        if(!finiteJson(args)) throw new BridgeException("invalid_value","Request contains a non-finite number");
        if(!requestSlots.tryAcquire()) throw new BridgeException("queue_full","Outstanding request count quota reached");
        String id=Long.toUnsignedString(ids.incrementAndGet());
        CompletableFuture<JsonElement> future=null;
        boolean registered=false;
        try {
            JsonObject req=new JsonObject();req.addProperty("id",id);req.addProperty("op",op);req.add("args",args);
            byte[] frame=(req+"\n").getBytes(StandardCharsets.UTF_8);
            if(frame.length>MAX_FRAME)throw new BridgeException("frame_too_large","Request exceeds 8 MiB");
            if(pendingBytes.addAndGet(frame.length)>MAX_PENDING_BYTES){pendingBytes.addAndGet(-frame.length);throw new BridgeException("queue_full","Outstanding request byte quota reached");}
            pendingFrameBytes.put(id,frame.length);
            future=new CompletableFuture<>();pending.put(id,future);registered=true;
            if(closed) throw new BridgeException("connection_closed","App is closed");
            synchronized(writeLock){input.write(frame);input.flush();}
            return future.get(timeoutSeconds,TimeUnit.SECONDS).getAsJsonObject();
        } catch(ExecutionException error) {
            Throwable cause=error.getCause();
            if(cause instanceof BridgeException bridge)throw bridge;
            throw new BridgeException("connection_closed",cause==null?"Host request failed":cause.getMessage());
        } catch(IOException error) {
            throw new BridgeException("connection_closed",error.getMessage());
        } catch(TimeoutException e) { throw new BridgeException("timeout","Request timed out"); }
        finally { if(!registered||pending.remove(id,future)){requestSlots.release();Integer bytes=pendingFrameBytes.remove(id);if(bytes!=null)pendingBytes.addAndGet(-bytes);} }
    }
    private static boolean finiteJson(JsonElement value) {
        if(value==null||value.isJsonNull())return true;
        if(value.isJsonPrimitive())return !value.getAsJsonPrimitive().isNumber()||Double.isFinite(value.getAsDouble());
        if(value.isJsonArray()){for(JsonElement child:value.getAsJsonArray())if(!finiteJson(child))return false;return true;}
        for(var entry:value.getAsJsonObject().entrySet())if(!finiteJson(entry.getValue()))return false;
        return true;
    }
    public Subscription on(String node,String event,Consumer<JsonObject> callback)throws Exception{
        String prefix=node+"\0"+event+"\0";String provisional=prefix+"pending-"+UUID.randomUUID();EventHandler handler=new EventHandler(callback);handlers.put(provisional,handler);
        try { JsonObject args=new JsonObject();args.addProperty("node",node);args.addProperty("event",event);String id=request("event.subscribe",args).get("subscription").getAsString();String key=prefix+id;handlers.remove(provisional,handler);handlers.put(key,handler);return new Subscription(this,id,key); }
        catch(Exception error){handlers.remove(provisional,handler);handler.active=false;throw error;}
    }
    void unsubscribe(String id,String key)throws Exception{EventHandler handler=handlers.remove(key);if(handler!=null)handler.active=false;JsonObject args=new JsonObject();args.addProperty("subscription",id);request("event.unsubscribe",args);}
    public Window createWindow(String title,double width,double height)throws Exception{JsonObject a=new JsonObject();a.addProperty("title",title);a.addProperty("width",width);a.addProperty("height",height);JsonObject r=request("window.create",a);return new Window(this,r.get("window").getAsString(),r.get("root").getAsString());}
    public Node createNode(Window w,String widget,JsonObject properties)throws Exception{JsonObject a=new JsonObject();a.addProperty("window",w.handle());a.addProperty("widget",widget);a.add("properties",properties);return new Node(this,request("node.create",a).get("node").getAsString());}
    public State createState(JsonObject value)throws Exception{JsonObject a=new JsonObject();a.add("value",value);return new State(this,request("state.create",a).get("state").getAsString());}
    private void readLoop(InputStream in){ByteArrayOutputStream frame=new ByteArrayOutputStream();byte[] chunk=new byte[8192];try{int n;while((n=in.read(chunk))!=-1)for(int i=0;i<n;i++){if(frame.size()>=MAX_FRAME)throw new IOException("Frame too large");if(chunk[i]=='\n'){handle(JsonParser.parseString(frame.toString(StandardCharsets.UTF_8)).getAsJsonObject());frame.reset();}else frame.write(chunk[i]);}if(frame.size()!=0)throw new IOException("EOF before LF");}catch(Exception e){fail(e);}finally{fail(new IOException("Host output closed"));}}
    private void handle(JsonObject m){if(m.get("kind").getAsString().equals("response")){String id=m.get("id").getAsString();var f=pending.remove(id);if(f==null)return;requestSlots.release();Integer bytes=pendingFrameBytes.remove(id);if(bytes!=null)pendingBytes.addAndGet(-bytes);if(m.get("ok").getAsBoolean())f.complete(m.get("result"));else{JsonObject e=m.getAsJsonObject("error");f.completeExceptionally(new BridgeException(e.get("code").getAsString(),e.get("message").getAsString()));}}else if(m.get("kind").getAsString().equals("event")){String prefix=m.get("source").getAsString()+"\0"+m.get("event").getAsString()+"\0";List<EventHandler> listeners=new ArrayList<>();handlers.forEach((k,v)->{if(k.startsWith(prefix))listeners.add(v);});long bytes=m.toString().getBytes(StandardCharsets.UTF_8).length;for(var listener:listeners){int count=callbackCount.incrementAndGet();long queuedBytes=callbackBytes.addAndGet(bytes);if(count>1024||queuedBytes>16L*1024*1024){callbackCount.decrementAndGet();callbackBytes.addAndGet(-bytes);fail(new BridgeException("queue_full","Callback queue quota reached"));return;}try{callbacks.execute(()->{try{if(listener.active)listener.callback.accept(m);}catch(Throwable error){try{errorHandler.accept(error);}catch(Throwable ignored){}}finally{callbackCount.decrementAndGet();callbackBytes.addAndGet(-bytes);}});}catch(RejectedExecutionException error){callbackCount.decrementAndGet();callbackBytes.addAndGet(-bytes);fail(error);return;}}}else fail(new IOException("Unknown message kind"));}
    private void failPending(Throwable error){pending.forEach((id,f)->{if(pending.remove(id,f)){requestSlots.release();Integer bytes=pendingFrameBytes.remove(id);if(bytes!=null)pendingBytes.addAndGet(-bytes);f.completeExceptionally(new BridgeException("connection_closed",error.getMessage()));}});}
    private void fail(Throwable t){if(closed)return;closed=true;failPending(t);process.destroy();}
    @Override public void close(){if(!closed)try{request("shutdown",new JsonObject(),5);}catch(Exception ignored){}failPending(new IOException("App closed"));closed=true;try{input.close();}catch(IOException ignored){}try{if(!process.waitFor(5,TimeUnit.SECONDS)){process.destroy();if(!process.waitFor(1,TimeUnit.SECONDS))process.destroyForcibly();}}catch(InterruptedException e){Thread.currentThread().interrupt();process.destroyForcibly();}callbacks.shutdownNow();}
}
