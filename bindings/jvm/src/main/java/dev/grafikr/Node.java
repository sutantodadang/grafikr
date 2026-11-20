package dev.grafikr;
import com.google.gson.JsonObject;
public record Node(App app,String handle) { public void attach(String parent)throws Exception{var a=new JsonObject();a.addProperty("node",handle);a.addProperty("parent",parent);app.request("node.attach",a);} public void update(JsonObject props)throws Exception{var a=new JsonObject();a.addProperty("node",handle);a.add("properties",props);app.request("property.update",a);}public void destroy()throws Exception{var a=new JsonObject();a.addProperty("node",handle);app.request("node.destroy",a);} }
