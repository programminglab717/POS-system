package dev.keel.android

import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.assertTextContains
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
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
        /** The demo café's small latte, and the oat milk among its milks. */
        val SMALL_LATTE = "button:${demoId(0x402)}"
        val OAT_MILK = "modifier:${demoId(0x202)}"
    }

    @get:Rule
    val compose = createAndroidComposeRule<MainActivity>()

    @Test
    fun a_latte_with_oat_milk_is_rung_paid_and_printed() {
        val (port, received) = printer(jobs = 2)
        compose.tap("member:Alex", wait = 20_000)
        compose.enterPin("1234")
        compose.tap("printer")
        compose.waitFor("printer:host")
        compose.onNodeWithTag("printer:host").performTextReplacement("127.0.0.1")
        compose.onNodeWithTag("printer:port").performTextReplacement(port.toString())
        compose.tap("printer:save")
        compose.tap("back")
        compose.tap(SMALL_LATTE)
        compose.tap(OAT_MILK)
        compose.tap("sheet:add")
        compose.tap("pay")
        compose.tap("offer:0")
        compose.waitFor("change")
        compose.onNodeWithTag("change").assertTextContains("$0.00", substring = true)
        val (drawer, receipt) = received()
        assertTrue(drawer.toList().windowed(2).any { it == listOf<Byte>(0x1B, 0x70) }, "the drawer kick first")
        assertTrue(String(receipt, Charsets.ISO_8859_1).contains("Keel Caf"), "then the receipt")
        compose.tap("orders")
        compose.waitFor("back")
        compose.waitUntil(10_000) { compose.onAllNodes(hasText("Paid")).fetchSemanticsNodes().isNotEmpty() }
    }

    /**
     * A cashier drops an order with an item on it, which needs a manager (ADR-0029): the register
     * asks who approves, takes their PIN, and drops it.
     */
    @Test
    fun a_cashier_drops_an_order_once_a_manager_approves() {
        compose.tap("member:Sam", wait = 20_000)
        compose.enterPin("2468")
        compose.tap("button:${demoId(0x401)}")
        compose.tap("sheet:add")
        compose.tap("abandon")
        compose.waitFor("approval:reason")
        compose.tap("approver:Alex")
        compose.enterPin("1234")
        compose.waitUntil(10_000) { compose.onAllNodes(hasTestTag("abandon")).fetchSemanticsNodes().isEmpty() }
        compose.tap("sign-out")
    }

    /**
     * A cashier takes 20% off an order, beyond their role's 10% (ADR-0030): a manager approves,
     * and the total is $3.25 less 20%, $2.60, with $0.23 of tax at 8.875%.
     */
    @Test
    fun a_cashier_discounts_an_order_once_a_manager_approves() {
        compose.tap("member:Sam", wait = 20_000)
        compose.enterPin("2468")
        compose.tap("button:${demoId(0x401)}")
        compose.tap("sheet:add")
        compose.tap("discount:order")
        compose.tap("reason:loyalty")
        compose.tap("percent:2000")
        compose.waitFor("approval:reason")
        compose.tap("approver:Alex")
        compose.enterPin("1234")
        compose.waitUntil(10_000) { compose.shown("total").contains("$2.83") }
        // Dropped, as the next test begins with no order open.
        compose.tap("abandon")
        compose.waitFor("approval:reason")
        compose.tap("approver:Alex")
        compose.enterPin("1234")
        compose.waitUntil(10_000) { compose.onAllNodes(hasTestTag("abandon")).fetchSemanticsNodes().isEmpty() }
        compose.tap("sign-out")
    }

    /**
     * A cashier prices an espresso by hand at $2.50, which needs a manager (ADR-0030), and notes
     * it: $2.50 with $0.22 of tax at 8.875%.
     */
    @Test
    fun a_cashier_prices_a_line_by_hand_once_a_manager_approves() {
        compose.tap("member:Sam", wait = 20_000)
        compose.enterPin("2468")
        compose.tap("button:${demoId(0x401)}")
        compose.tap("sheet:add")
        compose.tapStarting("price:0")
        compose.tap("price-reason:damaged")
        compose.tap("price:digit:2")
        compose.tap("price:digit:5")
        compose.tap("price:digit:0")
        compose.tap("price:set")
        compose.waitFor("approval:reason")
        compose.tap("approver:Alex")
        compose.enterPin("1234")
        compose.waitUntil(10_000) { compose.shown("total").contains("$2.72") }
        compose.tapStarting("note:0")
        compose.waitFor("notes:text")
        compose.onNodeWithTag("notes:text").performTextReplacement("extra hot")
        compose.tap("notes:save")
        compose.waitUntil(10_000) { compose.onAllNodes(tagStarting("notes:0")).fetchSemanticsNodes().isNotEmpty() }
        compose.tap("abandon")
        compose.waitFor("approval:reason")
        compose.tap("approver:Alex")
        compose.enterPin("1234")
        compose.waitUntil(10_000) { compose.onAllNodes(hasTestTag("abandon")).fetchSemanticsNodes().isEmpty() }
        compose.tap("sign-out")
    }

    /**
     * Two espressos split two ways (ADR-0030): each check $3.25 with $0.29 of tax, paid one after
     * the other, the order closing with the second.
     */
    @Test
    fun a_split_order_is_paid_check_by_check() {
        compose.tap("member:Alex", wait = 20_000)
        compose.enterPin("1234")
        compose.tap("button:${demoId(0x401)}")
        compose.tap("sheet:add")
        compose.tap("button:${demoId(0x401)}")
        compose.tap("sheet:add")
        compose.tap("split:2")
        compose.waitFor("check:2")
        compose.waitUntil(10_000) { compose.shown("check:1").contains("$3.54") }
        compose.tap("pay:check:1")
        compose.tap("offer:0")
        compose.waitUntil(10_000) { compose.shown("check:1").contains("(paid)") }
        compose.tap("pay:check:2")
        compose.tap("offer:0")
        // Closed with its last check: the register is ready for the next order. Found in review:
        // waiting for the change passed on the first check's.
        compose.waitUntil(10_000) { compose.onAllNodes(hasTestTag("check:1")).fetchSemanticsNodes().isEmpty() }
        compose.tap("sign-out")
    }

    /**
     * A manager opens the drawer with a $100.00 float, sells an espresso in the exact cash, $3.54,
     * and closes the drawer blind, counting $103.54: exact (ADR-0031). The day's X report reads.
     * The drawer is left closed, as the other tests begin with none open; while it is open, the
     * drawer page shows nothing from which what it holds is a sum.
     */
    @Test
    fun a_drawer_takes_a_cash_sale_and_closes_exactly() {
        compose.tap("member:Alex", wait = 20_000)
        compose.enterPin("1234")
        compose.tap("drawer:open")
        for (digit in "10000") compose.tap("cash:digit:$digit")
        compose.waitUntil(10_000) { compose.shown("cash:typed").contains("$100.00") }
        compose.tap("cash:confirm")
        compose.tap("button:${demoId(0x401)}")
        compose.tap("sheet:add")
        compose.tap("pay")
        compose.tap("offer:0")
        compose.waitFor("change")
        compose.tap("drawer")
        compose.waitFor("drawer:close")
        assertTrue(compose.onAllNodes(hasTestTag("closed:taken")).fetchSemanticsNodes().isEmpty(), "blind while open")
        compose.tap("drawer:close")
        for (digit in "10354") compose.tap("cash:digit:$digit")
        compose.waitUntil(10_000) { compose.shown("cash:typed").contains("$103.54") }
        compose.tap("cash:confirm")
        compose.waitUntil(10_000) { compose.shown("closed:variance").contains("$0.00") }
        compose.waitUntil(10_000) { compose.shown("closed:taken").contains("$3.54") }
        compose.tap("report:x")
        compose.waitFor("report:total")
        compose.tap("back")
        compose.tap("sign-out")
    }

    /**
     * A manager sells an espresso in the exact cash, $3.54, finds it by its receipt's code, and
     * returns it, refunded what was paid, nothing left of it after (ADR-0032). Found in review: the
     * test checked only the amount, which another test's espresso would have refunded too.
     */
    @Test
    fun a_paid_espresso_is_returned_and_refunded() {
        compose.tap("member:Alex", wait = 20_000)
        compose.enterPin("1234")
        compose.tap("button:${demoId(0x401)}")
        compose.tap("sheet:add")
        compose.tap("pay")
        compose.tap("offer:0")
        compose.waitFor("change")
        compose.tap("orders")
        // The latest paid order is first: this one. Its code is its identifier's last six.
        compose.waitUntil(10_000) { compose.onAllNodes(tagStarting("return:0")).fetchSemanticsNodes().isNotEmpty() }
        val tag = compose.onAllNodes(tagStarting("return:0")).fetchSemanticsNodes()[0].config[SemanticsProperties.TestTag]
        val order = tag.removePrefix("return:")
        compose.onNodeWithTag("return:code").performTextReplacement(order.takeLast(6).uppercase())
        compose.tap("return:find")
        compose.tapStarting("return-more:")
        compose.tap("return-reason:damaged")
        compose.tap("return:refund")
        compose.waitUntil(10_000) { compose.shown("return:handed").contains("$3.54") }
        compose.waitUntil(10_000) { compose.onAllNodes(hasText("0 left", substring = true)).fetchSemanticsNodes().isNotEmpty() }
        compose.tap("back")
        compose.waitFor("return:code")
        compose.tap("back")
        compose.tap("sign-out")
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
}
