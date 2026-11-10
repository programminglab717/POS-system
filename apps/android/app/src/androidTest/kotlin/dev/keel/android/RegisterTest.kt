package dev.keel.android

import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.ComposeTimeoutException
import androidx.compose.ui.test.assertTextContains
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The app on a device (ADR-0027): the kernel opens with keys from the Keystore; a cashier signs
 * in from the demo café's team, rings a small latte with oat milk through its modifier sheet, pays
 * it with the exact cash offered, and finds it paid among today's orders.
 */
@RunWith(AndroidJUnit4::class)
class RegisterTest {
    private companion object {
        fun id(n: Int) = "0192f0c1-0000-7000-8000-%012x".format(n)

        /** The demo café's small latte, and the oat milk among its milks. */
        val SMALL_LATTE = "button:${id(0x402)}"
        val OAT_MILK = "modifier:${id(0x202)}"
    }

    @get:Rule
    val compose = createAndroidComposeRule<MainActivity>()

    @Test
    fun a_latte_with_oat_milk_is_rung_paid_and_found_among_todays_orders() {
        tap("member:Alex", wait = 20_000)
        tap(SMALL_LATTE)
        tap(OAT_MILK)
        tap("sheet:add")
        tap("pay")
        tap("offer:0")
        waitFor("change")
        compose.onNodeWithTag("change").assertTextContains("$0.00", substring = true)
        tap("orders")
        waitFor("back")
        compose.waitUntil(10_000) { compose.onAllNodes(hasText("Paid")).fetchSemanticsNodes().isNotEmpty() }
    }

    /** Waits for the node tagged [tag], then taps it. */
    private fun tap(tag: String, wait: Long = 10_000) {
        waitFor(tag, wait)
        compose.onNodeWithTag(tag).performClick()
    }

    /** Waits for the node tagged [tag], failing with what the register shows if it never comes. */
    private fun waitFor(tag: String, timeoutMillis: Long = 10_000) {
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
