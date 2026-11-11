package dev.keel.android

import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.ComposeTimeoutException
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.junit4.ComposeTestRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick

/** The demo café's identifier ending in [n]. */
fun demoId(n: Int) = "0192f0c1-0000-7000-8000-%012x".format(n)

/** Waits for the node tagged [tag], failing with what the register shows if it never comes. */
fun ComposeTestRule.waitFor(tag: String, timeoutMillis: Long = 10_000) {
    try {
        waitUntil(timeoutMillis) { onAllNodes(hasTestTag(tag)).fetchSemanticsNodes().isNotEmpty() }
    } catch (timedOut: ComposeTimeoutException) {
        throw AssertionError("nothing tagged $tag within $timeoutMillis ms; the register shows: \"${shown("error")}\"", timedOut)
    }
}

/** Waits for the node tagged [tag], then taps it. */
fun ComposeTestRule.tap(tag: String, wait: Long = 10_000) {
    waitFor(tag, wait)
    onNodeWithTag(tag).performClick()
}

/** The text of the nodes tagged [tag], joined. */
fun ComposeTestRule.shown(tag: String): String = onAllNodes(hasTestTag(tag)).fetchSemanticsNodes()
    .flatMap { it.config.getOrElse(SemanticsProperties.Text) { emptyList() } }
    .joinToString()

/** Types [pin] on the PIN pad, and enters it. */
fun ComposeTestRule.enterPin(pin: String) {
    for (digit in pin) tap("pin:$digit")
    tap("pin:enter")
}
