package dev.keel.android

import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.ComposeTimeoutException
import androidx.compose.ui.test.assertTextContains
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextReplacement
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.io.ByteArrayOutputStream
import java.net.ServerSocket
import kotlin.concurrent.thread
import kotlin.test.assertTrue

/**
 * The app on a device (ADR-0027): the kernel opens with keys from the Keystore; a cashier signs
 * in from the demo café's team, points the register at a printer, rings a small latte with oat
 * milk through its modifier sheet, pays it with the exact cash offered, and the drawer kick and
 * the receipt reach the printer; and the order is paid among today's orders.
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
    fun a_latte_with_oat_milk_is_rung_paid_and_printed() {
        val (port, received) = printer(jobs = 2)
        tap("member:Alex", wait = 20_000)
        tap("printer")
        waitFor("printer:host")
        compose.onNodeWithTag("printer:host").performTextReplacement("127.0.0.1")
        compose.onNodeWithTag("printer:port").performTextReplacement(port.toString())
        tap("printer:save")
        tap("back")
        tap(SMALL_LATTE)
        tap(OAT_MILK)
        tap("sheet:add")
        tap("pay")
        tap("offer:0")
        waitFor("change")
        compose.onNodeWithTag("change").assertTextContains("$0.00", substring = true)
        val (drawer, receipt) = received()
        assertTrue(drawer.toList().windowed(2).any { it == listOf<Byte>(0x1B, 0x70) }, "the drawer kick first")
        assertTrue(String(receipt, Charsets.ISO_8859_1).contains("Keel Caf"), "then the receipt")
        tap("orders")
        waitFor("back")
        compose.waitUntil(10_000) { compose.onAllNodes(hasText("Paid")).fetchSemanticsNodes().isNotEmpty() }
    }

    /**
     * A printer in the test's process that takes [jobs] connections, one after the other, each
     * answered as a ready Epson that printed what it was sent; and what each brought, waited for.
     */
    private fun printer(jobs: Int): Pair<Int, () -> List<ByteArray>> {
        val server = ServerSocket(0)
        val received = mutableListOf<ByteArray>()
        val worker = thread {
            repeat(jobs) {
                server.accept().use { socket ->
                    val bytes = ByteArrayOutputStream()
                    val input = socket.getInputStream()
                    val output = socket.getOutputStream()
                    var answered = 0
                    val buf = ByteArray(4096)
                    while (true) {
                        val n = input.read(buf)
                        if (n < 0) break
                        bytes.write(buf, 0, n)
                        val all = bytes.toByteArray()
                        val questions = (0 until all.size - 1).count { all[it] == 0x10.toByte() && all[it + 1] == 0x04.toByte() }
                        while (answered < questions) {
                            output.write(0x12)
                            answered++
                        }
                        val ask = byteArrayOf(0x1D, 0x28, 0x48, 6, 0, 48, 48)
                        val at = (0..all.size - ask.size).firstOrNull { i -> ask.indices.all { all[i + it] == ask[it] } }
                        if (at != null && all.size >= at + 11) {
                            output.write(byteArrayOf(0x37, 0x22) + all.copyOfRange(at + 7, at + 11) + byteArrayOf(0))
                            output.flush()
                            break
                        }
                    }
                    received += bytes.toByteArray()
                }
            }
            server.close()
        }
        return server.localPort to {
            worker.join(30_000)
            check(!worker.isAlive) { "the printer had ${received.size} of $jobs jobs within 30 s" }
            received
        }
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
