package dev.grafikr;
import com.google.gson.JsonObject;
public record State(App app,String handle) { public JsonObject get()throws Exception{var a=new JsonObject();a.addProperty("state",handle);return app.request("state.get",a);}public void set(JsonObject value)throws Exception{var a=new JsonObject();a.addProperty("state",handle);a.add("value",value);app.request("state.set",a);}public void close()throws Exception{var a=new JsonObject();a.addProperty("state",handle);app.request("state.destroy",a);} }
