package dev.grafikr.example;

import com.google.gson.JsonObject;
import dev.grafikr.App;
import java.nio.file.Path;

public final class CounterForm {
    public static void main(String[] args) throws Exception {
        try (var app = new App(Path.of(System.getenv("GRAFIKR_HOST")))) {
            var window = app.createWindow("Counter and form", 480, 320);
            var label = app.createNode(window, "Text", props("text", "Count: 0"));
            label.attach(window.root());
            var button = app.createNode(window, "Button", props("label", "Add"));
            button.attach(window.root());
            var count = app.createState(stateValue("0"));
            app.on(button.handle(), "click", event -> {
                try {
                    long value = Long.parseLong(count.get().get("value").getAsString());
                    if (value == Long.MAX_VALUE) return;
                    count.set(stateValue(Long.toString(value + 1)));
                    label.update(props("text", "Count: " + (value + 1)));
                } catch (Exception error) {
                    throw new RuntimeException(error);
                }
            });
            var field = app.createNode(window, "TextField", props("text", "Grafikr 雪", "width", 320));
            field.attach(window.root());
            var preview = app.createNode(window, "Text", props("text", "Grafikr 雪"));
            preview.attach(window.root());
            app.on(field.handle(), "change", event -> {
                if (event.has("payload") && event.getAsJsonObject("payload").has("text")) {
                    try { preview.update(props("text", event.getAsJsonObject("payload").get("text").getAsString())); }
                    catch (Exception error) { throw new RuntimeException(error); }
                }
            });
            Thread.sleep(Long.MAX_VALUE);
        }
    }

    private static JsonObject props(Object... pairs) {
        JsonObject object = new JsonObject();
        for (int i = 0; i < pairs.length; i += 2) {
            String key = pairs[i].toString();
            Object value = pairs[i + 1];
            if (value instanceof Number number) object.addProperty(key, number);
            else if (value instanceof Boolean flag) object.addProperty(key, flag);
            else object.addProperty(key, value.toString());
        }
        return object;
    }

    private static JsonObject stateValue(String value) {
        var object = props("type", "i64");
        object.addProperty("value", value);
        return object;
    }
}
