package dev.grafikr

import com.google.gson.JsonObject
import java.nio.file.Path

fun main() = App(Path.of(System.getenv("GRAFIKR_HOST"))).use { app ->
    fun props(name: String, value: String) = JsonObject().apply { addProperty(name, value) }
    fun stateValue(value: String) = JsonObject().apply { addProperty("type", "i64"); addProperty("value", value) }
    val window = app.createWindow("Counter and form", 480.0, 320.0)
    val label = app.createNode(window, "Text", props("text", "Count: 0")); label.attach(window.root())
    val button = app.createNode(window, "Button", props("label", "Add")); button.attach(window.root())
    val count = app.createState(stateValue("0"))
    app.on(button.handle(), "click") {
        val value = count.get().get("value").asString.toLong()
        if (value < Long.MAX_VALUE) {
            count.set(stateValue((value + 1).toString()))
            label.update(props("text", "Count: ${value + 1}"))
        }
    }
    val field = app.createNode(window, "TextField", JsonObject().apply { addProperty("text", "Grafikr 雪"); addProperty("width", 320) })
    field.attach(window.root())
    val preview = app.createNode(window, "Text", props("text", "Grafikr 雪")); preview.attach(window.root())
    app.on(field.handle(), "change") { event ->
        event.getAsJsonObject("payload")?.get("text")?.asString?.let { preview.update(props("text", it)) }
    }
    Thread.sleep(Long.MAX_VALUE)
}
