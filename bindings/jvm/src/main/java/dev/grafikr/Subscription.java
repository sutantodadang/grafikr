package dev.grafikr;
public final class Subscription implements AutoCloseable { private final App app;private final String id,key;private boolean closed;Subscription(App a,String i,String k){app=a;id=i;key=k;}@Override public void close()throws Exception{if(!closed){closed=true;app.unsubscribe(id,key);}} }
