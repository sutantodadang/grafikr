package dev.grafikr;
public record Window(App app,String handle,String root) implements AutoCloseable { @Override public void close()throws Exception{var a=new com.google.gson.JsonObject();a.addProperty("window",handle);app.request("window.close",a);} }
