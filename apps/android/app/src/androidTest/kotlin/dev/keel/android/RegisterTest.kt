package dev.keel.android

import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.assertTextContains
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.ComposeTimeoutException
import androidx.compose.ui.test.hasTestTag
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The app on a device (ADR-0027): the kernel opens with keys from the Keystore, the demo café's
 * menu shows, and an item rung is paid in cash.
 */
@RunWith(AndroidJUnit4::class)
class RegisterTest {
    private companion object {
        /** The demo café's espresso, the first button of its first page. */
        const val ESPRESSO = "button:0192f0c1-0000-7000-8000-000000000401"
    }

    @get:Rule
    val compose = createAndroidComposeRule<MainActivity>()

    @Test
    fun an_espresso_is_rung_and_paid() {
        waitFor(ESPRESSO, 20_000)
        compose.onNodeWithTag(ESPRESSO).performClick()
        waitFor("pay", 10_000)
        compose.onNodeWithTag("pay").performClick()
        waitFor("change", 10_000)
        compose.onNodeWithTag("change").assertTextContains("$0.00", substring = true)
    }

    /** Waits for the node tagged [tag], failing with what the register shows if it never comes. */
    private fun waitFor(tag: String, timeoutMillis: Long) {
        try {
            compose.waitUntil(timeoutMillis) { compose.onAllNodes(hasTestTag(tag)).fetchSemanticsNodes().isNotEmpty() }
        } catch (timedOut: ComposeTimeoutException) {
            val shown = compose.onAllNodes(hasTestTag("error")).fetchSemanticsNodes()
                .flatMap { it.config.getOrElse(SemanticsProperties.Text) { emptyList() } }
                .joinToString()
            throw AssertionError("nothing tagged $tag within $timeoutMillis ms; the register shows: \"$shown\"", timedOut)
        }
    }
}
