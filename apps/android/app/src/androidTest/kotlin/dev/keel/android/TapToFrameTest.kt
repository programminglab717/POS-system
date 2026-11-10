package dev.keel.android

import android.util.Log
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The register's tap to frame (ADR-0023, decision 8; ADR-0027, part 5): 200 taps of a button,
 * each timed from the tap until the order on screen shows its line, through the kernel on its
 * thread and back. The median and the 95th and 99th percentiles go to the log, tagged
 * `keel-perf`; the test fails only if the 95th passes a bound far beyond the register's budget,
 * so it runs on slow emulators.
 * The reference device, with a 100,000-SKU catalog, gives the numbers that count.
 */
@RunWith(AndroidJUnit4::class)
class TapToFrameTest {
    @get:Rule
    val compose = createAndroidComposeRule<MainActivity>()

    @Test
    fun a_tap_shows_its_line_within_the_budget() {
        compose.tap("member:Alex", wait = 20_000)
        // Bottled water, which has nothing to choose, on the Food page.
        compose.waitUntil(10_000) { compose.onAllNodes(hasText("Food")).fetchSemanticsNodes().isNotEmpty() }
        compose.onAllNodes(hasText("Food"))[0].performClick()
        val water = "button:${demoId(0x409)}"
        compose.waitFor(water)
        val times = LongArray(TAPS)
        for (tap in 0 until TAPS) {
            // Each tap rings a line, and the order's total changes: the frame that shows it is the
            // one the tap waits for.
            val before = compose.shown("total")
            val started = System.nanoTime()
            compose.onNodeWithTag(water).performClick()
            compose.waitUntil(5_000) {
                val total = compose.shown("total")
                total.isNotEmpty() && total != before
            }
            times[tap] = System.nanoTime() - started
        }
        times.sort()
        // The sample at or above which the given share of the taps fall.
        fun at(share: Int) = times[(TAPS * share + 99) / 100 - 1] / 1_000_000.0
        val (median, p95, p99) = Triple(at(50), at(95), at(99))
        Log.i("keel-perf", "tap to frame over $TAPS taps: median %.1f ms, 95th %.1f ms, 99th %.1f ms".format(median, p95, p99))
        // The 95th, not the slowest: one stall of an emulator isn't the register's. Found in
        // review: with 100 taps, the 99th percentile was the slowest tap.
        check(p95 < 1_000) { "a tap's 95th percentile was $p95 ms" }
    }

    private companion object {
        const val TAPS = 200
    }
}
