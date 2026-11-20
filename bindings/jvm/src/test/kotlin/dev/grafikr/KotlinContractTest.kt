package dev.grafikr

import com.google.gson.JsonObject
import java.nio.file.Path
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.Assertions.assertTrue

class KotlinContractTest {
    @Test
    fun nestedRequestFromKotlinEventCallbackCompletes() {
        val suffix = if (System.getProperty("os.name").startsWith("Windows")) ".exe" else ""
        val host = Path.of("../../target/debug/fixture-host$suffix").toAbsolutePath()
        App(host).use { app ->
            val window = app.createWindow("Kotlin contract", 320.0, 240.0)
            val button = app.createNode(window, "Button", JsonObject())
            button.attach(window.root())
            val completed = CountDownLatch(1)
            app.on(button.handle(), "click") {
                val state = JsonObject().apply {
                    addProperty("type", "i64")
                    addProperty("value", "42")
                }
                val args = JsonObject().apply { add("value", state) }
                app.request("state.create", args)
                completed.countDown()
            }.use { subscription ->
                assertTrue(completed.await(2, TimeUnit.SECONDS), "Kotlin callback did not complete nested request")
                subscription.close()
            }
            window.close()
        }
    }
}
