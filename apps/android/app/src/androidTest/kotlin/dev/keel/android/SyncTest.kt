package dev.keel.android

import androidx.compose.ui.test.assertTextContains
import androidx.compose.ui.test.junit4.v2.createEmptyComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File

/**
 * A provisioned register on a device (ADR-0026, ADR-0027): the kit CI provisions, with its hub
 * running on the host, is imported on the app's first launch; the register joins its location
 * over TLS, hears the hub, and sells while synced. Without a kit among the test's assets, as
 * outside CI, it is skipped.
 */
@RunWith(AndroidJUnit4::class)
class SyncTest {
    @get:Rule
    val compose = createEmptyComposeRule()

    @Test
    fun a_provisioned_register_syncs_with_its_hub_and_sells() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val assets = instrumentation.context.assets
        val names = assets.list("kit").orEmpty()
        assumeTrue("no kit among the test's assets: CI provisions one", names.isNotEmpty())
        // Where the app imports a kit from, on its first launch.
        val kit = File(instrumentation.targetContext.getExternalFilesDir(null), "kit").apply { mkdirs() }
        for (name in names) {
            assets.open("kit/$name").use { input -> File(kit, name).outputStream().use(input::copyTo) }
        }
        ActivityScenario.launch(MainActivity::class.java).use {
            compose.tap("member:Alex", wait = 30_000)
            compose.enterPin("1234")
            compose.waitUntil(60_000) { compose.shown("sync") == "Synced" }
            check(!kit.exists()) { "the kit was imported and removed" }
            compose.tap("button:${demoId(0x402)}")
            compose.tap("modifier:${demoId(0x202)}")
            compose.tap("sheet:add")
            compose.tap("pay")
            compose.tap("offer:0")
            compose.waitFor("change")
            compose.onNodeWithTag("change").assertTextContains("$0.00", substring = true)
            compose.waitUntil(10_000) { compose.shown("sync") == "Synced" }
        }
    }
}
