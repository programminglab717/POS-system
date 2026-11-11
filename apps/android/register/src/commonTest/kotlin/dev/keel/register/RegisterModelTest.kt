package dev.keel.register

import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull

class RegisterModelTest {
    private fun TestScope.model(till: Till) = RegisterModel(till, this, StandardTestDispatcher(testScheduler))

    /** [model], started and signed in as Alex. */
    private fun TestScope.signedIn(till: Till = FakeTill()): RegisterModel {
        val model = model(till)
        model.start()
        testScheduler.advanceUntilIdle()
        model.signIn(model.state.value.team.first())
        testScheduler.advanceUntilIdle()
        return model
    }

    private fun RegisterModel.button(name: String) = state.value.pages.flatMap { it.buttons }.single { it.name == name }

    @Test
    fun a_cashier_signs_in_from_the_team_and_out_again() = runTest {
        val till = FakeTill()
        val model = model(till)
        assertEquals(Screen.SIGN_IN, model.state.value.screen)
        model.start()
        testScheduler.advanceUntilIdle()
        assertEquals(listOf("Alex", "Sam"), model.state.value.team.map { it.name })
        model.signIn(Member("sam", "Sam"))
        testScheduler.advanceUntilIdle()
        assertEquals(Screen.REGISTER, model.state.value.screen)
        assertEquals("sam", till.signedIn)
        assertEquals(listOf("Drinks", "Food"), model.state.value.pages.map { it.name })
        model.signOut()
        testScheduler.advanceUntilIdle()
        assertEquals(Screen.SIGN_IN, model.state.value.screen)
        assertNull(till.signedIn)
        assertEquals(2, model.state.value.team.size, "the team stays for the next cashier")
    }

    @Test
    fun an_item_with_choices_opens_its_sheet_and_rings_what_was_chosen() = runTest {
        val till = FakeTill()
        val model = signedIn(till)
        model.tap(model.button("Latte"))
        testScheduler.advanceUntilIdle()
        val sheet = assertNotNull(model.state.value.sheet)
        assertEquals(false, sheet.complete, "a milk must be chosen first")
        val (milk, shots) = sheet.shown
        model.choose(milk.index, "oat")
        model.choose(shots.index, "shot")
        model.choose(shots.index, "shot")
        model.sheetQuantity(2)
        model.addFromSheet()
        testScheduler.advanceUntilIdle()
        assertNull(model.state.value.sheet)
        assertEquals(listOf("latte" to listOf(Chosen("oat", 1), Chosen("shot", 2))), till.rung)
        assertEquals("2", model.state.value.ticket!!.lines.single().quantity)
    }

    @Test
    fun lines_change_and_come_off() = runTest {
        val model = signedIn()
        model.tap(model.button("Tea"))
        testScheduler.advanceUntilIdle()
        model.tap(model.button("Scone"))
        testScheduler.advanceUntilIdle()
        val (tea, scone) = model.state.value.ticket!!.lines
        model.more(tea)
        testScheduler.advanceUntilIdle()
        assertEquals("$8.75", model.state.value.ticket!!.total)
        model.fewer(model.state.value.ticket!!.lines.first())
        testScheduler.advanceUntilIdle()
        model.fewer(scone)
        testScheduler.advanceUntilIdle()
        assertEquals(listOf("tea"), model.state.value.ticket!!.lines.map { it.name }, "the last one takes the line off")
        model.remove(model.state.value.ticket!!.lines.single())
        testScheduler.advanceUntilIdle()
        assertEquals(emptyList(), model.state.value.ticket!!.lines)
        model.abandon()
        testScheduler.advanceUntilIdle()
        assertNull(model.state.value.ticket)
    }

    @Test
    fun cash_is_tendered_from_an_offer_or_typed_and_short_is_refused() = runTest {
        val model = signedIn()
        model.tap(model.button("Tea"))
        testScheduler.advanceUntilIdle()
        model.tap(model.button("Scone"))
        testScheduler.advanceUntilIdle()
        model.openTender()
        testScheduler.advanceUntilIdle()
        val tender = assertNotNull(model.state.value.tender)
        assertEquals("$5.75", tender.due.text)
        assertEquals(listOf("$5.75", "$6.00", "$10.00"), tender.offers.map { it.text })
        // Typed: 5, 0, 0, then one erased: $0.50, short.
        for (digit in listOf(5, 0, 0)) model.typeDigit(digit)
        testScheduler.advanceUntilIdle()
        assertEquals("$5.00", model.state.value.tender!!.typed!!.text)
        model.eraseDigit()
        testScheduler.advanceUntilIdle()
        model.pay(model.state.value.tender!!.typed!!)
        testScheduler.advanceUntilIdle()
        assertEquals("That's short: $5.75 is due.", model.state.value.error)
        assertNotNull(model.state.value.tender, "the tender stays open")
        model.pay(model.state.value.tender!!.offers[2])
        testScheduler.advanceUntilIdle()
        assertNull(model.state.value.ticket)
        assertNull(model.state.value.tender)
        assertEquals("$4.25", model.state.value.change)
    }

    @Test
    fun todays_orders_show_and_an_open_one_is_taken_up_again() = runTest {
        val model = signedIn()
        model.tap(model.button("Tea"))
        testScheduler.advanceUntilIdle()
        model.openTender()
        testScheduler.advanceUntilIdle()
        model.pay(model.state.value.tender!!.offers[0])
        testScheduler.advanceUntilIdle()
        model.tap(model.button("Scone"))
        testScheduler.advanceUntilIdle()
        model.showOrders()
        testScheduler.advanceUntilIdle()
        assertEquals(Screen.ORDERS, model.state.value.screen)
        val (paid, open) = model.state.value.orders
        assertEquals(listOf(OrderState.CLOSED, OrderState.OPEN), listOf(paid.state, open.state))
        model.resume(paid)
        testScheduler.advanceUntilIdle()
        assertEquals("That order isn't open.", model.state.value.error)
        model.resume(open)
        testScheduler.advanceUntilIdle()
        assertEquals(Screen.REGISTER, model.state.value.screen)
        assertEquals(listOf("scone"), model.state.value.ticket!!.lines.map { it.name })
    }

    @Test
    fun a_refusal_is_shown_until_the_next_intent() = runTest {
        val model = model(FakeTill())
        model.start()
        testScheduler.advanceUntilIdle()
        model.signIn(Member("kim", "Kim"))
        testScheduler.advanceUntilIdle()
        assertEquals("no such member", model.state.value.error)
        model.signIn(Member("alex", "Alex"))
        testScheduler.advanceUntilIdle()
        assertNull(model.state.value.error)
    }

    @Test
    fun a_failure_that_isnt_a_refusal_is_shown_not_thrown() = runTest {
        val broken = object : Till by FakeTill() {
            override fun team(): List<Member> = throw IllegalStateException("the store is gone")
        }
        val model = model(broken)
        model.start()
        testScheduler.advanceUntilIdle()
        assertEquals("Something went wrong: the store is gone", model.state.value.error)
        assertEquals(false, model.state.value.loading)
    }

    @Test
    fun a_failure_with_no_message_is_shown_by_its_name() = runTest {
        class StoreInUse : Exception("")
        val broken = object : Till by FakeTill() {
            override fun team(): List<Member> = throw StoreInUse()
        }
        val model = model(broken)
        model.start()
        testScheduler.advanceUntilIdle()
        assertEquals("Something went wrong: StoreInUse", model.state.value.error)
    }

    /** Printer settings kept in memory. */
    private class Kept(var address: PrinterAddress? = null) : PrinterSettings {
        override fun load() = address
        override fun save(address: PrinterAddress) {
            this.address = address
        }
    }

    @Test
    fun the_printers_address_is_kept_and_checked() = runTest {
        val kept = Kept(PrinterAddress("10.0.0.5"))
        val model = RegisterModel(FakeTill(), this, StandardTestDispatcher(testScheduler), kept)
        model.start()
        testScheduler.advanceUntilIdle()
        assertEquals(PrinterAddress("10.0.0.5", 9100), model.state.value.printer, "loaded at the start")
        model.setPrinter(" printer.local ", "9101")
        testScheduler.advanceUntilIdle()
        assertEquals(PrinterAddress("printer.local", 9101), kept.address)
        for ((host, port) in listOf("" to "9100", "a b" to "9100", "printer" to "0", "printer" to "65536", "printer" to "x")) {
            model.setPrinter(host, port)
            testScheduler.advanceUntilIdle()
            assertNotNull(model.state.value.error, "$host:$port refused")
        }
        assertEquals(PrinterAddress("printer.local", 9101), kept.address, "nothing refused was kept")
    }

    @Test
    fun a_paid_order_is_reprinted_and_an_unconfirmed_job_resolved() = runTest {
        val till = FakeTill()
        val model = signedIn(till)
        model.tap(model.button("Tea"))
        testScheduler.advanceUntilIdle()
        model.openTender()
        testScheduler.advanceUntilIdle()
        model.pay(model.state.value.tender!!.offers[0])
        testScheduler.advanceUntilIdle()
        model.showOrders()
        testScheduler.advanceUntilIdle()
        val paid = model.state.value.orders.single()
        model.reprint(paid)
        testScheduler.advanceUntilIdle()
        assertEquals(listOf(JobKind.DRAWER, JobKind.RECEIPT, JobKind.RECEIPT), model.state.value.jobs.map { it.kind })
        // A printer that took the receipt and fell silent: staff say whether it printed.
        till.jobs[1] = till.jobs[1].copy(state = JobState.UNCONFIRMED)
        model.showPrinter()
        testScheduler.advanceUntilIdle()
        assertEquals(Screen.PRINTER, model.state.value.screen)
        model.resolve(model.state.value.jobs[1], printed = false)
        testScheduler.advanceUntilIdle()
        assertEquals(listOf(JobState.QUEUED, JobState.QUEUED, JobState.QUEUED), model.state.value.jobs.map { it.state })
        model.resolve(model.state.value.jobs[0], printed = true)
        testScheduler.advanceUntilIdle()
        assertEquals("That job isn't waiting for a decision.", model.state.value.error)
        // An order not paid has no receipt.
        model.tap(model.button("Scone"))
        testScheduler.advanceUntilIdle()
        model.reprint(model.state.value.ticket!!)
        testScheduler.advanceUntilIdle()
        assertEquals("Only a paid order has a receipt.", model.state.value.error)
    }

    @Test
    fun how_replication_stands_is_read_again_and_a_failure_to_read_it_shown_as_nothing() = runTest {
        var info: SyncInfo? = SyncInfo(listening = false, settled = true, hubReachable = true, serving = false)
        var broken = false
        val till = object : Till by FakeTill() {
            override fun syncState(): SyncInfo? = if (broken) error("no store") else info
        }
        val model = model(till)
        model.refreshSync()
        testScheduler.advanceUntilIdle()
        assertEquals(info, model.state.value.sync)
        info = info!!.copy(hubReachable = false)
        model.refreshSync()
        testScheduler.advanceUntilIdle()
        assertEquals(false, model.state.value.sync!!.hubReachable)
        broken = true
        model.refreshSync()
        testScheduler.advanceUntilIdle()
        assertNull(model.state.value.sync)
        assertNull(model.state.value.error, "no error for the cashier")
    }

    @Test
    fun a_profile_the_cloud_publishes_brings_the_new_menu_and_team() = runTest {
        var version = "v1"
        var pages = listOf("Drinks", "Food")
        var team = listOf(Member("alex", "Alex"), Member("sam", "Sam"))
        val fake = FakeTill()
        val till = object : Till by fake {
            override fun profileVersion() = version
            override fun menu() = fake.menu().take(pages.size).mapIndexed { n, page -> page.copy(name = pages[n]) }
            override fun team() = team
        }
        val model = signedIn(till)
        assertEquals(listOf("Drinks", "Food"), model.state.value.pages.map { it.name })
        model.refreshSync()
        testScheduler.advanceUntilIdle()
        assertEquals("v1", model.state.value.profileVersion)
        // Published: the menu is read again, and the cashier stays signed in.
        version = "v2"
        pages = listOf("Coffee")
        model.refreshSync()
        testScheduler.advanceUntilIdle()
        assertEquals(listOf("Coffee"), model.state.value.pages.map { it.name })
        assertEquals("v2", model.state.value.profileVersion)
        assertEquals(Screen.REGISTER, model.state.value.screen)
        assertEquals(0, model.state.value.page)
        // Nothing new: nothing read again.
        pages = listOf("Never")
        model.refreshSync()
        testScheduler.advanceUntilIdle()
        assertEquals(listOf("Coffee"), model.state.value.pages.map { it.name })
        // A version taken while the register can't say how replication stands, as before it joins
        // its network: read again all the same. Found in review.
        version = "v2b"
        pages = listOf("Coffee", "Tea")
        model.refreshSync()
        testScheduler.advanceUntilIdle()
        assertEquals(listOf("Coffee", "Tea"), model.state.value.pages.map { it.name })
        pages = listOf("Coffee")
        // A team without Alex: the kernel signed them out, and the register asks who is working.
        version = "v3"
        team = listOf(Member("sam", "Sam"))
        model.refreshSync()
        testScheduler.advanceUntilIdle()
        assertEquals(Screen.SIGN_IN, model.state.value.screen)
        assertNull(model.state.value.member)
        assertEquals(listOf("Sam"), model.state.value.team.map { it.name })
        assertEquals("v3", model.state.value.profileVersion)
    }

    @Test
    fun a_member_with_a_pin_signs_in_once_it_is_typed() = runTest {
        val fake = FakeTill().apply { pins = mapOf("alex" to "1234", "sam" to "2468") }
        val till = object : Till by fake {
            override fun team() = listOf(Member("alex", "Alex", "Manager", true), Member("sam", "Sam", "Cashier", true))
        }
        val model = model(till)
        model.start()
        testScheduler.advanceUntilIdle()
        model.signIn(model.state.value.team[1])
        assertEquals("sam", model.state.value.pinFor?.id, "the PIN pad, for Sam")
        assertNull(fake.signedIn)
        model.enterPin("1234")
        testScheduler.advanceUntilIdle()
        assertEquals("That PIN isn't right.", model.state.value.error)
        assertEquals(Screen.SIGN_IN, model.state.value.screen)
        assertEquals("sam", model.state.value.pinFor?.id, "still asking")
        model.enterPin("2468")
        testScheduler.advanceUntilIdle()
        assertEquals(Screen.REGISTER, model.state.value.screen)
        assertNull(model.state.value.pinFor)
        assertEquals("sam", fake.signedIn)
        // Cancelled, no one is asked for.
        model.signOut()
        testScheduler.advanceUntilIdle()
        model.signIn(model.state.value.team[0])
        model.cancelPin()
        assertNull(model.state.value.pinFor)
    }

    @Test
    fun a_manager_approves_and_the_refused_intent_goes_through() = runTest {
        val till = FakeTill().apply {
            pins = mapOf("alex" to "1234", "sam" to "2468")
            voidNeedsApproval = true
        }
        val model = model(till)
        model.start()
        testScheduler.advanceUntilIdle()
        model.signIn(Member("sam", "Sam", "Cashier", hasPin = true))
        model.enterPin("2468")
        testScheduler.advanceUntilIdle()
        assertEquals(Screen.REGISTER, model.state.value.screen)
        model.tap(model.button("Tea"))
        testScheduler.advanceUntilIdle()
        model.abandon()
        testScheduler.advanceUntilIdle()
        val asked = model.state.value.approval
        assertEquals("order.void", asked?.action)
        assertEquals("Dropping it needs a manager.", asked?.reason)
        assertNull(model.state.value.error, "an approval asked for, not an error")
        assertNotNull(model.state.value.ticket, "the order is still there")
        assertEquals(listOf("alex"), model.approvers().map { it.id }, "anyone but the cashier")
        model.chooseApprover(model.approvers().first())
        model.approve("0000")
        testScheduler.advanceUntilIdle()
        assertEquals("That PIN isn't right.", model.state.value.error)
        assertNotNull(model.state.value.approval, "still asking")
        model.approve("1234")
        testScheduler.advanceUntilIdle()
        assertNull(model.state.value.approval)
        assertNull(model.state.value.ticket, "dropped once approved")
        assertNull(model.state.value.error)
        // Cancelled, the order stays.
        model.tap(model.button("Tea"))
        testScheduler.advanceUntilIdle()
        model.abandon()
        testScheduler.advanceUntilIdle()
        model.cancelApproval()
        assertNull(model.state.value.approval)
        assertNotNull(model.state.value.ticket)
    }

    /** Found in review: two taps before the screen showed the first counted from the same line. */
    @Test
    fun taps_queued_together_each_count_from_the_one_before() = runTest {
        val model = signedIn()
        model.tap(model.button("Tea"))
        testScheduler.advanceUntilIdle()
        val tea = model.state.value.ticket!!.lines.single()
        // Three taps of + from the same screen, then two of −, before any answer is shown.
        repeat(3) { model.more(tea) }
        testScheduler.advanceUntilIdle()
        assertEquals("4", model.state.value.ticket!!.lines.single().quantity)
        val four = model.state.value.ticket!!.lines.single()
        repeat(4) { model.fewer(four) }
        testScheduler.advanceUntilIdle()
        assertEquals(emptyList(), model.state.value.ticket!!.lines, "the fourth took the line off")
        assertNull(model.state.value.error, "nothing refused")
    }

    @Test
    fun signing_out_keeps_the_open_order_for_the_next_cashier() = runTest {
        val model = signedIn()
        model.tap(model.button("Tea"))
        testScheduler.advanceUntilIdle()
        val order = model.state.value.ticket!!.order
        model.signOut()
        testScheduler.advanceUntilIdle()
        assertEquals(order, model.state.value.ticket?.order)
        model.signIn(Member("sam", "Sam"))
        testScheduler.advanceUntilIdle()
        model.tap(model.button("Scone"))
        testScheduler.advanceUntilIdle()
        assertEquals(order, model.state.value.ticket!!.order, "rung on the same order")
        assertEquals(2, model.state.value.ticket!!.lines.size)
    }
}
