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

    /** Found in review: refused again once approved, the next approval replayed the first PIN. */
    @Test
    fun an_intent_refused_again_is_tried_again_without_the_first_approval() = runTest {
        val fake = FakeTill().apply { voidNeedsApproval = true }
        val approvals = mutableListOf<Pair<String, String>>()
        val till = object : Till by fake {
            override fun approve(approver: String, pin: String) {
                approvals += approver to pin
            }
            override fun abandon(order: String): Ticket =
                throw TillException("Dropping it needs a manager.", approval = "order.void")
        }
        val model = signedIn(till)
        model.tap(model.button("Tea"))
        testScheduler.advanceUntilIdle()
        model.abandon()
        testScheduler.advanceUntilIdle()
        model.chooseApprover(model.approvers().first())
        model.approve("1234")
        testScheduler.advanceUntilIdle()
        assertNotNull(model.state.value.approval, "refused again: asked again")
        model.chooseApprover(model.approvers().first())
        model.approve("5678")
        testScheduler.advanceUntilIdle()
        assertEquals(listOf("alex" to "1234", "alex" to "5678"), approvals, "never the first PIN again")
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

    @Test
    fun a_discount_goes_on_a_line_or_the_order_and_comes_off() = runTest {
        val till = FakeTill()
        val model = signedIn(till)
        // No order, no sheet.
        model.openDiscount()
        assertNull(model.state.value.discount)
        model.tap(model.button("Tea"))
        model.tap(model.button("Scone"))
        testScheduler.advanceUntilIdle()
        val tea = model.state.value.ticket!!.lines[0]
        model.openDiscount(tea)
        model.discountReason("staff")
        assertEquals(DiscountSheet(tea, "staff"), model.state.value.discount)
        model.discountPercent(1_000)
        testScheduler.advanceUntilIdle()
        assertNull(model.state.value.discount, "the sheet closes once it is on")
        val ticket = model.state.value.ticket!!
        assertEquals(listOf("Staff 10%"), ticket.lines[0].discounts.map { it.words })
        assertEquals("−$0.30", ticket.lines[0].discounts[0].amount)
        assertEquals("$5.45", ticket.total)
        // An amount typed as a cash keypad types it.
        model.openDiscount(model.state.value.ticket!!.lines[1])
        model.typeDiscountDigit(5)
        model.typeDiscountDigit(0)
        model.typeDiscountDigit(1)
        model.eraseDiscountDigit()
        testScheduler.advanceUntilIdle()
        assertEquals(50L, model.state.value.discount?.typed?.minor)
        model.discountAmount()
        testScheduler.advanceUntilIdle()
        assertEquals("$4.95", model.state.value.ticket!!.total)
        assertEquals(DiscountAsked.Amount(50), till.asked.last().second)
        // A whole order takes a percentage, never an amount.
        model.openDiscount()
        model.typeDiscountDigit(1)
        testScheduler.advanceUntilIdle()
        model.discountAmount()
        testScheduler.advanceUntilIdle()
        assertEquals("A whole order takes a percentage off.", model.state.value.error)
        model.discountPercent(2_000)
        testScheduler.advanceUntilIdle()
        val discounted = model.state.value.ticket!!
        assertEquals(listOf("Loyalty 20%"), discounted.discounts.map { it.words })
        assertEquals("$3.96", discounted.total)
        // Off again.
        model.removeDiscount(discounted.discounts[0])
        testScheduler.advanceUntilIdle()
        assertEquals("$4.95", model.state.value.ticket!!.total)
        assertEquals(emptyList(), model.state.value.ticket!!.discounts)
    }

    @Test
    fun a_discount_beyond_the_cashier_waits_for_a_manager_and_goes_on_as_asked() = runTest {
        val till = FakeTill().apply {
            pins = mapOf("alex" to "1234")
            discountLimit = 1_000
        }
        // Sam, a cashier, signs in; Alex approves.
        val model = signedIn(object : Till by till {
            override fun team() = listOf(Member("sam", "Sam"), Member("alex", "Alex", "Manager", true))
        })
        model.tap(model.button("Tea"))
        testScheduler.advanceUntilIdle()
        val tea = model.state.value.ticket!!.lines[0]
        model.openDiscount(tea)
        model.discountReason("damaged")
        model.discountPercent(5_000)
        testScheduler.advanceUntilIdle()
        assertEquals("discount.apply", model.state.value.approval?.action)
        assertNull(model.state.value.ticket!!.lines[0].discounts.firstOrNull())
        // The sheet is closed meanwhile: the retry asks for what was asked, as it was.
        model.closeDiscount()
        model.chooseApprover(model.approvers().single())
        model.approve("1234")
        testScheduler.advanceUntilIdle()
        assertNull(model.state.value.approval)
        assertEquals(listOf("Damaged 50%"), model.state.value.ticket!!.lines[0].discounts.map { it.words })
        assertEquals(listOf<Pair<String?, DiscountAsked>>(tea.line to DiscountAsked.Percent(5_000)), till.asked)
    }

    @Test
    fun percentages_read_as_people_write_them() {
        assertEquals("10%", percentWords(1_000))
        assertEquals("12.5%", percentWords(1_250))
        assertEquals("0.01%", percentWords(1))
        assertEquals("100%", percentWords(10_000))
        assertEquals("33.33%", percentWords(3_333))
    }

    @Test
    fun a_line_is_priced_by_hand_once_a_manager_approves() = runTest {
        val till = FakeTill().apply {
            pins = mapOf("alex" to "1234")
            priceNeedsApproval = true
        }
        val model = signedIn(object : Till by till {
            override fun team() = listOf(Member("sam", "Sam"), Member("alex", "Alex", "Manager", true))
        })
        model.tap(model.button("Tea"))
        testScheduler.advanceUntilIdle()
        model.openPrice(model.state.value.ticket!!.lines[0])
        model.priceReason("damaged")
        for (digit in listOf(2, 5, 0)) model.typePriceDigit(digit)
        testScheduler.advanceUntilIdle()
        assertEquals("$2.50", model.state.value.price?.typed?.text)
        model.setPrice()
        testScheduler.advanceUntilIdle()
        assertEquals("price.override", model.state.value.approval?.action)
        model.chooseApprover(model.approvers().single())
        model.approve("1234")
        testScheduler.advanceUntilIdle()
        assertNull(model.state.value.price, "the sheet closes once it is priced")
        val line = model.state.value.ticket!!.lines[0]
        assertEquals("$2.50 (was $3.00), damaged", line.repriced)
        assertEquals("$2.50", line.amount)
        assertEquals("$2.50", model.state.value.ticket!!.total)
    }

    @Test
    fun a_lines_notes_are_kept_and_cleared() = runTest {
        val till = FakeTill()
        val model = signedIn(till)
        model.tap(model.button("Tea"))
        testScheduler.advanceUntilIdle()
        model.openNotes(model.state.value.ticket!!.lines[0])
        model.saveNotes("  oat milk on the side ")
        testScheduler.advanceUntilIdle()
        assertNull(model.state.value.notes)
        assertEquals("oat milk on the side", model.state.value.ticket!!.lines[0].notes)
        // The same notes change nothing, and ask the till nothing.
        model.openNotes(model.state.value.ticket!!.lines[0])
        model.saveNotes("oat milk on the side")
        testScheduler.advanceUntilIdle()
        assertNull(model.state.value.error)
        // Blank clears them.
        model.openNotes(model.state.value.ticket!!.lines[0])
        model.saveNotes("   ")
        testScheduler.advanceUntilIdle()
        assertNull(model.state.value.ticket!!.lines[0].notes)
    }

    @Test
    fun a_split_order_is_paid_check_by_check_part_by_part() = runTest {
        val till = FakeTill()
        val model = signedIn(till)
        model.tap(model.button("Tea"))
        model.tap(model.button("Scone"))
        testScheduler.advanceUntilIdle()
        model.split(2)
        testScheduler.advanceUntilIdle()
        val checks = model.state.value.ticket!!.checks
        // $3.00 and $2.75 split two ways: $2.88 and $2.87.
        assertEquals(listOf("$2.88", "$2.87"), checks.map { it.total })
        // Part of the first, typed as a cash keypad types it, in exact cash.
        model.openTender(checks[0])
        testScheduler.advanceUntilIdle()
        assertEquals("$2.88", model.state.value.tender?.due?.text)
        model.typeDigit(1)
        model.typeDigit(0)
        model.typeDigit(0)
        testScheduler.advanceUntilIdle()
        model.payPart()
        testScheduler.advanceUntilIdle()
        val ticket = model.state.value.ticket!!
        assertEquals("$1.88", ticket.checks[0].due, "the order stays, with what each check owes now")
        assertEquals(OrderState.OPEN, ticket.state)
        // The rest of the first, then the second: the order closes with the last.
        model.openTender(ticket.checks[0])
        testScheduler.advanceUntilIdle()
        model.pay(model.state.value.tender!!.offers[0])
        testScheduler.advanceUntilIdle()
        assertEquals(false, model.state.value.ticket!!.checks[0].open)
        model.openTender(model.state.value.ticket!!.checks[1])
        testScheduler.advanceUntilIdle()
        assertEquals(listOf("$2.87", "$5.00", "$10.00", "$20.00"), model.state.value.tender!!.offers.map { it.text })
        model.pay(model.state.value.tender!!.offers[1])
        testScheduler.advanceUntilIdle()
        assertNull(model.state.value.ticket, "paid in full: the next order begins")
        assertEquals("$2.13", model.state.value.change)
    }

    @Test
    fun a_line_moves_to_a_check_of_its_own() = runTest {
        val till = FakeTill()
        val model = signedIn(till)
        model.tap(model.button("Tea"))
        model.tap(model.button("Scone"))
        testScheduler.advanceUntilIdle()
        model.moveLine(model.state.value.ticket!!.lines[1], null)
        testScheduler.advanceUntilIdle()
        val checks = model.state.value.ticket!!.checks
        assertEquals(listOf(listOf("tea"), listOf("scone")), checks.map { check -> check.lines.map { it.name } })
        model.moveLine(model.state.value.ticket!!.lines[1], checks[0].check)
        testScheduler.advanceUntilIdle()
        assertEquals(emptyList(), model.state.value.ticket!!.checks, "back on one check: none listed")
    }

    @Test
    fun an_order_parked_on_another_register_is_recalled_through_the_hub() = runTest {
        val till = FakeTill().apply { hubAnswers = false }
        val model = signedIn(till)
        model.tap(model.button("Tea"))
        testScheduler.advanceUntilIdle()
        val order = model.state.value.ticket!!.order
        // Another register owns it.
        till.owners[order] = "front"
        model.showOrders()
        testScheduler.advanceUntilIdle()
        val parked = model.state.value.orders.single()
        assertEquals(true, model.elsewhere(parked))
        // Opening it asks the hub; it stays in the list until the hub grants it.
        model.resume(parked)
        testScheduler.advanceUntilIdle()
        assertEquals(Screen.ORDERS, model.state.value.screen)
        assertEquals(listOf("here"), model.state.value.orders.single().asking)
        // Granted, it opens on this register.
        till.owners[order] = "here"
        model.resume(model.state.value.orders.single())
        testScheduler.advanceUntilIdle()
        assertEquals(Screen.REGISTER, model.state.value.screen)
        assertEquals(order, model.state.value.ticket!!.order)
    }

    @Test
    fun an_order_is_taken_over_once_a_manager_approves() = runTest {
        val till = FakeTill().apply {
            pins = mapOf("alex" to "1234")
            voidNeedsApproval = true
        }
        val model = signedIn(object : Till by till {
            override fun team() = listOf(Member("sam", "Sam"), Member("alex", "Alex", "Manager", true))
        })
        model.tap(model.button("Tea"))
        testScheduler.advanceUntilIdle()
        val order = model.state.value.ticket!!.order
        till.owners[order] = "front"
        model.showOrders()
        testScheduler.advanceUntilIdle()
        model.takeOver(model.state.value.orders.single())
        testScheduler.advanceUntilIdle()
        assertEquals("order.void", model.state.value.approval?.action)
        model.chooseApprover(model.approvers().single())
        model.approve("1234")
        testScheduler.advanceUntilIdle()
        assertEquals(Screen.REGISTER, model.state.value.screen)
        assertEquals("here", model.state.value.ticket!!.owner)
    }

    /** Found in review: a double tap of a percentage put on two discounts. */
    @Test
    fun a_double_tap_puts_on_one_discount() = runTest {
        val till = FakeTill()
        val model = signedIn(till)
        model.tap(model.button("Tea"))
        testScheduler.advanceUntilIdle()
        model.openDiscount(model.state.value.ticket!!.lines[0])
        model.discountPercent(1_000)
        model.discountPercent(1_000)
        testScheduler.advanceUntilIdle()
        assertEquals(1, till.asked.size)
        assertEquals(1, model.state.value.ticket!!.lines[0].discounts.size)
    }

    /** Found in review: an order parked before the day turned couldn't be recalled. */
    @Test
    fun an_open_order_from_an_earlier_day_is_listed() = runTest {
        val till = FakeTill()
        val model = signedIn(till)
        model.tap(model.button("Tea"))
        testScheduler.advanceUntilIdle()
        val order = model.state.value.ticket!!.order
        till.earlier += order
        model.showOrders()
        testScheduler.advanceUntilIdle()
        assertEquals(listOf(order), model.state.value.orders.map { it.order })
    }

    @Test
    fun the_drawer_opens_with_a_float_takes_cash_and_closes_blind() = runTest {
        val till = FakeTill()
        val model = signedIn(till)
        assertEquals(true, model.state.value.askDrawer, "no drawer open: the register asks")
        // A float of $100.00, typed.
        model.beginCash(CashPurpose.FLOAT)
        for (digit in listOf(1, 0, 0, 0, 0)) model.typeCashDigit(digit)
        testScheduler.advanceUntilIdle()
        model.confirmCash()
        testScheduler.advanceUntilIdle()
        assertEquals("$100.00", model.state.value.drawer?.float)
        assertEquals(false, model.state.value.askDrawer, "asked until a drawer opens")
        // A sale in cash goes into it.
        model.tap(model.button("Tea"))
        testScheduler.advanceUntilIdle()
        model.openTender()
        testScheduler.advanceUntilIdle()
        model.pay(model.state.value.tender!!.offers[0])
        testScheduler.advanceUntilIdle()
        model.showDrawer()
        testScheduler.advanceUntilIdle()
        assertEquals(Screen.DRAWER, model.state.value.screen)
        // Blind: neither what moved through it nor what it should hold. Found in review.
        assertNull(model.state.value.drawer?.taken, "the close is blind")
        assertNull(model.state.value.drawer?.expected, "the close is blind")
        // A drop of nothing is refused before the kernel sees it; the keypad stays.
        model.beginCash(CashPurpose.DROP)
        model.confirmCash()
        testScheduler.advanceUntilIdle()
        assertEquals("Type an amount.", model.state.value.error)
        assertEquals(CashPurpose.DROP, model.state.value.cash?.purpose)
        // $5.00 dropped, as a safe drop with no reason chosen. Found in review: it was change.
        for (digit in listOf(5, 0, 0)) model.typeCashDigit(digit)
        testScheduler.advanceUntilIdle()
        model.confirmCash()
        testScheduler.advanceUntilIdle()
        assertEquals("safe_drop", till.lastCashReason)
        // A blind count of $97.50, its close tapped before the digits are in: the whole count
        // closes the drawer, $0.50 short. Found in review: read at the tap, it could close short.
        model.beginCash(CashPurpose.COUNT)
        for (digit in listOf(9, 7, 5, 0)) model.typeCashDigit(digit)
        model.confirmCash()
        testScheduler.advanceUntilIdle()
        assertNull(model.state.value.drawer)
        val closed = model.state.value.closedDrawer!!
        assertEquals(Triple("$98.00", "$97.50", "-$0.50"), Triple(closed.expected, closed.counted, closed.variance))
        assertEquals(Pair("$3.00", "$5.00"), Pair(closed.taken, closed.dropped))
        // The Z report, once closed.
        model.showReport(z = true)
        testScheduler.advanceUntilIdle()
        assertEquals(true, model.state.value.report?.z)
        assertEquals(1, model.state.value.report?.drawers?.size)
        model.printReport(z = true)
        testScheduler.advanceUntilIdle()
        assertEquals(true, model.state.value.jobs.any { it.job == "report:z" })
    }

    @Test
    fun a_paid_out_beyond_the_cashier_waits_for_a_manager_and_goes_out_as_asked() = runTest {
        val till = FakeTill().apply {
            pins = mapOf("alex" to "1234")
            paidOutLimit = 0
        }
        val model = signedIn(object : Till by till {
            override fun team() = listOf(Member("sam", "Sam"), Member("alex", "Alex", "Manager", true))
        })
        model.beginCash(CashPurpose.FLOAT)
        model.confirmCash()
        testScheduler.advanceUntilIdle()
        assertEquals("$0.00", model.state.value.drawer?.float, "nothing typed is a float of nothing")
        model.beginCash(CashPurpose.PAID_OUT)
        model.cashReason("supplier")
        for (digit in listOf(2, 0, 0)) model.typeCashDigit(digit)
        testScheduler.advanceUntilIdle()
        model.confirmCash()
        testScheduler.advanceUntilIdle()
        assertEquals("drawer.paid_out", model.state.value.approval?.action)
        model.chooseApprover(model.approvers().single())
        model.approve("1234")
        testScheduler.advanceUntilIdle()
        assertNull(model.state.value.approval)
        assertEquals("supplier", till.lastCashReason)
        // The Z waits for the drawer to close, in the kernel's words.
        model.showReport(z = true)
        testScheduler.advanceUntilIdle()
        assertEquals("a drawer is open: close it first", model.state.value.error)
        model.beginCash(CashPurpose.COUNT)
        model.confirmCash()
        testScheduler.advanceUntilIdle()
        assertEquals("$2.00", model.state.value.closedDrawer?.paidOut)
    }

    /**
     * A close over or short beyond the cashier, and a no-sale, each wait for a manager, and go
     * ahead as asked once one approves (ADR-0031). A report read before is gone once the drawer
     * page shows again. Found in review: neither gate was tried, and a report stayed stale.
     */
    @Test
    fun a_short_close_and_a_no_sale_wait_for_a_manager() = runTest {
        val till = FakeTill().apply {
            pins = mapOf("alex" to "1234")
            varianceLimit = 500
            noSaleNeedsApproval = true
        }
        val model = signedIn(object : Till by till {
            override fun team() = listOf(Member("sam", "Sam"), Member("alex", "Alex", "Manager", true))
        })
        // A float cancelled: the register still asks for one.
        model.beginCash(CashPurpose.FLOAT)
        model.cancelCash()
        assertEquals(true, model.state.value.askDrawer)
        model.beginCash(CashPurpose.FLOAT)
        for (digit in listOf(1, 0, 0, 0, 0)) model.typeCashDigit(digit)
        model.confirmCash()
        testScheduler.advanceUntilIdle()
        model.showReport(z = false)
        testScheduler.advanceUntilIdle()
        assertNotNull(model.state.value.report)
        model.showDrawer()
        testScheduler.advanceUntilIdle()
        assertNull(model.state.value.report, "read again, it says what is so now")
        // A no-sale, for change for a customer, once a manager approves.
        model.beginCash(CashPurpose.NO_SALE)
        model.confirmCash()
        testScheduler.advanceUntilIdle()
        assertEquals("drawer.no_sale", model.state.value.approval?.action)
        model.chooseApprover(model.approvers().single())
        model.approve("1234")
        testScheduler.advanceUntilIdle()
        assertEquals(1L, model.state.value.drawer?.noSales)
        assertEquals("change_for_customer", till.lastCashReason)
        // $5.01 short of $100.00, beyond the cashier's $5.00: the same count, once approved.
        model.beginCash(CashPurpose.COUNT)
        for (digit in listOf(9, 4, 9, 9)) model.typeCashDigit(digit)
        model.confirmCash()
        testScheduler.advanceUntilIdle()
        assertEquals("drawer.variance", model.state.value.approval?.action)
        assertNotNull(model.state.value.drawer, "still open until a manager approves")
        model.chooseApprover(model.approvers().single())
        model.approve("1234")
        testScheduler.advanceUntilIdle()
        assertNull(model.state.value.drawer)
        assertEquals("-$5.01", model.state.value.closedDrawer?.variance)
    }

    /**
     * A paid order found from its receipt's code, its tea returned once a manager approves, for a
     * reason (ADR-0032): picked no more than is left, refunded, and nothing left after.
     */
    @Test
    fun a_paid_tea_is_returned_once_a_manager_approves() = runTest {
        val till = FakeTill().apply {
            pins = mapOf("alex" to "1234")
            refundLimit = 0
        }
        val model = signedIn(object : Till by till {
            override fun team() = listOf(Member("sam", "Sam"), Member("alex", "Alex", "Manager", true))
        })
        model.tap(model.button("Tea"))
        testScheduler.advanceUntilIdle()
        model.openTender()
        testScheduler.advanceUntilIdle()
        model.pay(model.state.value.tender!!.offers[0])
        testScheduler.advanceUntilIdle()
        // Not found by a code no receipt has; found by the end of its own.
        model.showOrders()
        testScheduler.advanceUntilIdle()
        model.findReturn("zzzzzz")
        testScheduler.advanceUntilIdle()
        assertEquals("No paid order has that code.", model.state.value.error)
        model.findReturn("ORDER1")
        testScheduler.advanceUntilIdle()
        assertEquals(Screen.RETURN, model.state.value.screen)
        val line = model.state.value.returning!!.returnable.lines.single()
        // Nothing picked: nothing to refund.
        model.confirmReturn()
        testScheduler.advanceUntilIdle()
        assertEquals("Choose what is returned.", model.state.value.error)
        // One tea, and no more than the one there is.
        model.pickMore(line)
        model.pickMore(line)
        assertEquals(mapOf(line.line to 1L), model.state.value.returning?.picks)
        // No reason chosen: it is asked for. Found in review: every return was "damaged".
        model.confirmReturn()
        testScheduler.advanceUntilIdle()
        assertEquals("Choose why it is returned.", model.state.value.error)
        model.returnReason("wrong_item")
        model.confirmReturn()
        testScheduler.advanceUntilIdle()
        assertEquals("refund.issue", model.state.value.approval?.action)
        model.chooseApprover(model.approvers().single())
        model.approve("1234")
        testScheduler.advanceUntilIdle()
        assertEquals("$3.00", model.state.value.refund?.handed)
        assertEquals("wrong_item", till.lastReturnReason)
        val after = model.state.value.returning!!
        assertEquals(emptyMap(), after.picks)
        assertEquals(false, after.returnable.lines.single().anyLeft)
        assertEquals("$3.00", after.returnable.refunded)
        // Back to today's orders.
        model.closeReturn()
        testScheduler.advanceUntilIdle()
        assertEquals(Screen.ORDERS, model.state.value.screen)
        assertNull(model.state.value.returning)
    }

    /** Two teas paid, and the model on the return of the order. */
    private suspend fun kotlinx.coroutines.test.TestScope.returning(till: FakeTill, model: RegisterModel = signedIn(till)): RegisterModel {
        model.tap(model.button("Tea"))
        testScheduler.advanceUntilIdle()
        model.more(model.state.value.ticket!!.lines.single())
        testScheduler.advanceUntilIdle()
        model.openTender()
        testScheduler.advanceUntilIdle()
        model.pay(model.state.value.tender!!.offers[0])
        testScheduler.advanceUntilIdle()
        model.findReturn("order1")
        testScheduler.advanceUntilIdle()
        return model
    }

    /** A second tap while the refund is made refunds nothing more. Found in review: it did. */
    @Test
    fun a_double_tap_refunds_once() = runTest {
        val till = FakeTill()
        val model = returning(till)
        val line = model.state.value.returning!!.returnable.lines.single()
        model.pickMore(line)
        model.returnReason("damaged")
        model.confirmReturn()
        model.confirmReturn()
        testScheduler.advanceUntilIdle()
        assertEquals("$3.00", model.state.value.refund?.amount)
        assertEquals(1, till.printJobs().count { it.job.startsWith("refund:") })
        assertEquals(1L, model.state.value.returning?.returnable?.lines?.single()?.leftCount)
    }

    /** A line sold by measure is picked whole. */
    @Test
    fun a_line_sold_by_measure_is_returned_whole() = runTest {
        val till = FakeTill().apply { measured = setOf("tea") }
        val model = returning(till)
        val line = model.state.value.returning!!.returnable.lines.single()
        assertNull(line.leftCount)
        model.pickMore(line)
        assertEquals(mapOf<String, Long?>(line.line to null), model.state.value.returning?.picks)
        model.returnReason("damaged")
        model.confirmReturn()
        testScheduler.advanceUntilIdle()
        assertEquals("$6.00", model.state.value.refund?.amount)
        assertEquals(false, model.state.value.returning?.returnable?.lines?.single()?.anyLeft)
    }

    /** An order a card paid isn't refunded in cash: nothing is picked. */
    @Test
    fun an_order_a_card_paid_isnt_refunded_in_cash() = runTest {
        val till = FakeTill().apply { cardPaid += "order1" }
        val model = returning(till)
        val sheet = model.state.value.returning!!
        assertEquals(false, sheet.returnable.cashOnly)
        model.pickMore(sheet.returnable.lines.single())
        assertEquals(emptyMap(), model.state.value.returning?.picks)
    }

    /**
     * A code of six characters finds an order, and one that two orders end with lists both to
     * choose from. Found in review: a shorter one opened whatever order it ended.
     */
    @Test
    fun a_receipts_code_finds_its_order_or_asks_which() = runTest {
        val till = FakeTill()
        val model = signedIn(object : Till by till {
            override fun findOrders(code: String) = if (code == "TWICE1") listOf("order1", "order2") else till.findOrders(code)
        })
        model.tap(model.button("Tea"))
        testScheduler.advanceUntilIdle()
        model.openTender()
        testScheduler.advanceUntilIdle()
        model.pay(model.state.value.tender!!.offers[0])
        testScheduler.advanceUntilIdle()
        model.tap(model.button("Tea"))
        testScheduler.advanceUntilIdle()
        model.openTender()
        testScheduler.advanceUntilIdle()
        model.pay(model.state.value.tender!!.offers[0])
        testScheduler.advanceUntilIdle()
        model.showOrders()
        testScheduler.advanceUntilIdle()
        model.findReturn("rder1")
        testScheduler.advanceUntilIdle()
        assertEquals("A receipt's code is six characters.", model.state.value.error)
        model.findReturn("TWICE1")
        testScheduler.advanceUntilIdle()
        assertEquals(listOf("order1", "order2"), model.state.value.matches?.map { it.order })
        assertEquals(Screen.ORDERS, model.state.value.screen)
        model.beginReturn(model.state.value.matches!![1])
        testScheduler.advanceUntilIdle()
        assertEquals("order2", model.state.value.returning?.returnable?.order)
        assertNull(model.state.value.matches)
    }

    @Test
    fun an_item_86d_from_the_menu_isnt_rung_until_it_is_put_back() = runTest {
        val till = FakeTill()
        val model = signedIn(till)
        assertEquals(emptyList(), model.state.value.soldOut)
        model.toggleMarking()
        model.tap(model.button("Tea"))
        testScheduler.advanceUntilIdle()
        assertEquals("tea", model.state.value.mark?.button?.variant)
        assertEquals(emptyList(), model.state.value.mark?.modifiers, "tea offers no modifiers")
        // Two quick taps make one 86.
        model.eightySix("tea", modifier = false, untilPutBack = false)
        model.eightySix("tea", modifier = false, untilPutBack = false)
        testScheduler.advanceUntilIdle()
        assertNull(model.state.value.error)
        assertEquals(1L, till.availability)
        assertEquals(listOf(SoldOutItem("tea", false, "Tea", "2026-10-03 04:00")), model.state.value.soldOut)
        assertEquals(true, model.button("Tea").soldOut)
        assertEquals(true, model.state.value.mark?.button?.soldOut, "the sheet shows it sold out")
        assertEquals(false, model.state.value.mark?.working)
        // Ringing it is refused at once, with no order begun.
        model.toggleMarking()
        assertNull(model.state.value.mark)
        model.tap(model.button("Tea"))
        testScheduler.advanceUntilIdle()
        assertEquals("Tea is sold out until 04:00.", model.state.value.error)
        assertNull(model.state.value.ticket)
        assertEquals(emptyList(), till.rung)
        // Put back, it rings.
        model.toggleMarking()
        model.tap(model.button("Tea"))
        testScheduler.advanceUntilIdle()
        model.putBack("tea", modifier = false)
        testScheduler.advanceUntilIdle()
        assertEquals(emptyList(), model.state.value.soldOut)
        assertEquals(false, model.button("Tea").soldOut)
        model.closeMark()
        model.toggleMarking()
        model.tap(model.button("Tea"))
        testScheduler.advanceUntilIdle()
        assertEquals(listOf("tea"), till.rung.map { it.first })
    }

    @Test
    fun an_items_86_sheet_lists_its_modifiers_each_86d_on_its_own() = runTest {
        val till = FakeTill()
        val model = signedIn(till)
        model.toggleMarking()
        model.tap(model.button("Latte"))
        testScheduler.advanceUntilIdle()
        val sheet = assertNotNull(model.state.value.mark)
        assertEquals(listOf("whole", "oat", "shot", "vanilla", "caramel", "double"), sheet.modifiers.map { it.modifier })
        assertNull(model.state.value.sheet, "no modifier sheet while marking")
        model.eightySix("oat", modifier = true, untilPutBack = true)
        testScheduler.advanceUntilIdle()
        assertEquals(listOf(SoldOutItem("oat", true, "Oat", null)), model.state.value.soldOut)
        val oat = model.state.value.mark?.modifiers?.single { it.modifier == "oat" }
        assertEquals(true, oat?.soldOut)
        assertEquals(null, oat?.backAt)
        assertEquals(false, model.button("Latte").soldOut, "the latte itself is on sale")
        assertEquals("until put back", backWords(null))
    }

    @Test
    fun an_86_from_another_register_reaches_the_menu_and_the_open_sheet() = runTest {
        val till = FakeTill()
        val model = signedIn(till)
        model.tap(model.button("Latte"))
        testScheduler.advanceUntilIdle()
        val milk = model.state.value.sheet!!.shown.first()
        model.choose(milk.index, "oat")
        // Another register 86s oat milk, and the scone; this one hears of it.
        till.sold["oat"] = "2026-10-03 04:00"
        till.sold["scone"] = null
        till.availability += 2
        model.refreshSync()
        testScheduler.advanceUntilIdle()
        val sheet = assertNotNull(model.state.value.sheet)
        assertEquals(0, sheet.count(milk.index, "oat"), "a choice now sold out is dropped")
        assertEquals(false, sheet.complete)
        model.choose(milk.index, "oat")
        assertEquals(0, model.state.value.sheet!!.count(milk.index, "oat"), "and can't be chosen again")
        model.choose(milk.index, "whole")
        assertEquals(true, model.state.value.sheet!!.complete)
        assertEquals(true, model.button("Scone").soldOut)
        assertEquals(2, model.state.value.soldOut.size)
        // Its time passes, which moves no version: read again all the same.
        till.sold.remove("oat")
        model.refreshSync()
        testScheduler.advanceUntilIdle()
        assertEquals(listOf("scone"), model.state.value.soldOut.map { it.item })
        assertEquals(false, model.state.value.sheet!!.item.groups.first().modifiers.single { it.modifier == "oat" }.soldOut)
    }

    @Test
    fun an_86_beyond_the_cashiers_role_asks_a_manager() = runTest {
        val till = FakeTill().apply { markNeedsApproval = true }
        val model = signedIn(till)
        model.toggleMarking()
        model.tap(model.button("Scone"))
        testScheduler.advanceUntilIdle()
        model.eightySix("scone", modifier = false, untilPutBack = false)
        testScheduler.advanceUntilIdle()
        assertEquals("item.availability", model.state.value.approval?.action)
        assertEquals(false, model.state.value.mark?.working, "the sheet can be tapped again")
        assertEquals(emptyList(), model.state.value.soldOut)
        model.chooseApprover(model.approvers().first())
        model.approve("")
        testScheduler.advanceUntilIdle()
        assertNull(model.state.value.approval)
        assertEquals(listOf("scone"), model.state.value.soldOut.map { it.item })
        // Refused by the item's own rules: it is sold out already.
        till.approvedBy = "alex"
        model.eightySix("scone", modifier = false, untilPutBack = false)
        testScheduler.advanceUntilIdle()
        assertEquals("the item is already sold out until then, or already on sale", model.state.value.error)
    }

    /** Found in review: a refresh put back the sheet it began with, losing what was chosen meanwhile. */
    @Test
    fun choices_made_while_availability_is_read_stay() = runTest {
        val fake = FakeTill()
        var model: RegisterModel? = null
        var chooseDuringRead = false
        val till = object : Till by fake {
            override fun soldOut(): List<SoldOutItem> {
                if (chooseDuringRead) {
                    chooseDuringRead = false
                    val shots = model!!.state.value.sheet!!.shown[1]
                    model!!.choose(shots.index, "shot")
                    model!!.sheetQuantity(3)
                }
                return fake.soldOut()
            }
        }
        model = signedIn(till)
        model.tap(model.button("Latte"))
        testScheduler.advanceUntilIdle()
        fake.sold["vanilla"] = null
        fake.availability += 1
        chooseDuringRead = true
        model.refreshSync()
        testScheduler.advanceUntilIdle()
        val sheet = model.state.value.sheet!!
        assertEquals(listOf(1, 3), listOf(sheet.count(sheet.shown[1].index, "shot"), sheet.quantity))
        assertEquals(true, sheet.item.groups.flatMap { it.modifiers }.single { it.modifier == "vanilla" }.soldOut)
    }

    /** Found in review: a tap made marking, then Done before the worker ran it, rang the item. */
    @Test
    fun a_tap_made_marking_never_rings() = runTest {
        val till = FakeTill()
        val model = signedIn(till)
        model.toggleMarking()
        model.tap(model.button("Tea"))
        model.toggleMarking()
        testScheduler.advanceUntilIdle()
        assertEquals(emptyList(), till.rung)
        assertNull(model.state.value.ticket)
    }

    /** Found in review: every 86 in the store cleared the error shown, and a failed read showed one. */
    @Test
    fun an_86_heard_from_elsewhere_keeps_the_error_shown_and_a_failed_read_shows_none() = runTest {
        val fake = FakeTill()
        var failing = false
        val till = object : Till by fake {
            override fun menu(): List<MenuPage> = if (failing) throw TillException("disk") else fake.menu()
        }
        val model = signedIn(till)
        fake.sold["tea"] = null
        fake.availability += 1
        model.refreshSync()
        testScheduler.advanceUntilIdle()
        model.tap(model.button("Tea"))
        testScheduler.advanceUntilIdle()
        assertEquals("Tea is sold out until put back.", model.state.value.error)
        fake.sold["scone"] = null
        fake.availability += 1
        model.refreshSync()
        testScheduler.advanceUntilIdle()
        assertEquals("Tea is sold out until put back.", model.state.value.error, "still shown")
        assertEquals(true, model.button("Scone").soldOut)
        failing = true
        fake.sold.remove("scone")
        fake.availability += 1
        model.refreshSync()
        testScheduler.advanceUntilIdle()
        assertEquals("Tea is sold out until put back.", model.state.value.error, "a failed read says nothing")
        failing = false
        model.refreshSync()
        testScheduler.advanceUntilIdle()
        assertEquals(false, model.button("Scone").soldOut, "read again at the next poll")
    }

    /** Found in review: an item 86'd with its sheet open left the sheet, its Add refused behind it. */
    @Test
    fun an_items_sheet_closes_when_the_item_sells_out() = runTest {
        val fake = FakeTill()
        val model = signedIn(fake)
        model.tap(model.button("Latte"))
        testScheduler.advanceUntilIdle()
        assertNotNull(model.state.value.sheet)
        fake.sold["latte"] = "2026-10-03 04:00"
        fake.availability += 1
        model.refreshSync()
        testScheduler.advanceUntilIdle()
        assertNull(model.state.value.sheet)
        assertEquals("Latte just sold out.", model.state.value.error)
    }

    /** Found in review: the 86 sheet's buttons came back before it showed what the change did. */
    @Test
    fun the_86_sheet_works_until_it_shows_the_change() = runTest {
        val fake = FakeTill()
        var model: RegisterModel? = null
        val working = mutableListOf<Boolean?>()
        val till = object : Till by fake {
            override fun soldOut(): List<SoldOutItem> {
                working += model?.state?.value?.mark?.working
                return fake.soldOut()
            }
        }
        model = signedIn(till)
        model.toggleMarking()
        model.tap(model.button("Tea"))
        testScheduler.advanceUntilIdle()
        working.clear()
        model.eightySix("tea", modifier = false, untilPutBack = false)
        testScheduler.advanceUntilIdle()
        assertEquals(listOf<Boolean?>(true), working, "still working while it reads")
        assertEquals(false, model.state.value.mark?.working)
        assertEquals(true, model.state.value.mark?.button?.soldOut)
    }

    /** Kept stations, as a device's settings keep them. */
    private class KeptStations(var kept: Set<String>? = null) : KitchenSettings {
        override fun load() = kept
        override fun save(stations: Set<String>) {
            kept = stations
        }
    }

    @Test
    fun an_order_is_sent_and_a_sent_line_is_voided_for_a_reason() = runTest {
        val till = FakeTill()
        val model = signedIn(till)
        assertEquals(3, model.state.value.stations.size, "the register knows its kitchen")
        model.tap(model.button("Tea"))
        model.tap(model.button("Scone"))
        testScheduler.advanceUntilIdle()
        model.send()
        testScheduler.advanceUntilIdle()
        val ticket = model.state.value.ticket!!
        assertEquals(listOf(true, true), ticket.lines.map { it.sent })
        // A sent line isn't changed: one more is refused, one fewer or off asks why it is voided.
        val tea = ticket.lines[0]
        model.more(tea)
        testScheduler.advanceUntilIdle()
        assertEquals("tea was sent to the kitchen: ring another.", model.state.value.error)
        model.fewer(tea)
        testScheduler.advanceUntilIdle()
        assertEquals(tea.line, model.state.value.voiding?.line)
        model.closeVoid()
        assertNull(model.state.value.voiding)
        model.remove(tea)
        testScheduler.advanceUntilIdle()
        // Two quick taps void it once.
        model.voidLine("customer")
        model.voidLine("customer")
        testScheduler.advanceUntilIdle()
        assertNull(model.state.value.error)
        assertEquals(listOf("scone"), model.state.value.ticket!!.lines.map { it.name })
        // A line rung after is sent on its own.
        model.tap(model.button("Tea"))
        testScheduler.advanceUntilIdle()
        assertEquals(listOf(true, false), model.state.value.ticket!!.lines.map { it.sent })
        model.send()
        testScheduler.advanceUntilIdle()
        assertEquals(listOf(true, true), model.state.value.ticket!!.lines.map { it.sent })
    }

    @Test
    fun a_void_beyond_the_cashiers_role_asks_a_manager() = runTest {
        val till = FakeTill().apply { lineVoidNeedsApproval = true }
        val model = signedIn(till)
        model.tap(model.button("Tea"))
        testScheduler.advanceUntilIdle()
        model.send()
        testScheduler.advanceUntilIdle()
        model.remove(model.state.value.ticket!!.lines[0])
        testScheduler.advanceUntilIdle()
        model.voidLine("wrong_item")
        testScheduler.advanceUntilIdle()
        assertEquals("line.remove", model.state.value.approval?.action)
        model.chooseApprover(model.approvers().first())
        model.approve("")
        testScheduler.advanceUntilIdle()
        assertNull(model.state.value.approval)
        assertEquals(emptyList(), model.state.value.ticket!!.lines)
    }

    @Test
    fun a_kitchen_screen_shows_its_stations_tickets_and_bumps_them_once() = runTest {
        val till = FakeTill()
        val settings = KeptStations()
        val model = RegisterModel(till, this, StandardTestDispatcher(testScheduler), kitchenSettings = settings)
        model.start()
        testScheduler.advanceUntilIdle()
        model.signIn(model.state.value.team.first())
        testScheduler.advanceUntilIdle()
        model.tap(model.button("Tea"))
        model.tap(model.button("Scone"))
        testScheduler.advanceUntilIdle()
        model.send()
        testScheduler.advanceUntilIdle()
        model.signOut()
        testScheduler.advanceUntilIdle()
        // No one signs in to the kitchen screen; it shows every station until some are kept.
        model.openKitchen()
        testScheduler.advanceUntilIdle()
        assertEquals(Screen.KITCHEN, model.state.value.screen)
        val kds = model.state.value.kds!!
        assertEquals(setOf("bar", "kitchen", "expo"), kds.stations)
        assertEquals(listOf("bar", "kitchen", "expo"), kds.view.tickets.map { it.station })
        // The bar shown alone, kept for next time.
        model.toggleStation("kitchen")
        model.toggleStation("expo")
        testScheduler.advanceUntilIdle()
        assertEquals(setOf("bar"), settings.kept)
        assertEquals(listOf("bar"), model.state.value.kds!!.view.tickets.map { it.station })
        model.toggleStation("bar")
        testScheduler.advanceUntilIdle()
        assertEquals(setOf("bar"), model.state.value.kds!!.stations, "one station is always shown")
        // Bumped twice quickly, by tap or key: once.
        model.bumpSelected()
        model.bumpSelected()
        testScheduler.advanceUntilIdle()
        assertEquals(1, till.bumpCalls)
        assertNull(model.state.value.error)
        assertEquals(emptyList(), model.state.value.kds!!.view.tickets)
        // Recalled, it is back.
        model.recallLast()
        testScheduler.advanceUntilIdle()
        assertEquals(1, model.state.value.kds!!.view.tickets.size)
        // Expo: the tea made, the scone not yet.
        model.toggleStation("expo")
        model.toggleStation("bar")
        testScheduler.advanceUntilIdle()
        val expo = model.state.value.kds!!.view.tickets.single()
        assertEquals(listOf(false, false), expo.items.map { it.ready })
        model.closeKitchen()
        assertEquals(Screen.SIGN_IN, model.state.value.screen)
        assertNull(model.state.value.kds)
        // Opened again, it shows the stations kept.
        model.openKitchen()
        testScheduler.advanceUntilIdle()
        assertEquals(setOf("expo"), model.state.value.kds!!.stations)
    }

    @Test
    fun a_kitchen_screens_selection_follows_its_keys_and_stays_on_its_ticket() = runTest {
        val till = FakeTill()
        val model = signedIn(till)
        for (name in listOf("Tea", "Scone")) {
            model.tap(model.button(name))
            testScheduler.advanceUntilIdle()
            // Each sent on its own: the scone's sending sends the scone alone.
            model.send()
            testScheduler.advanceUntilIdle()
        }
        model.signOut()
        testScheduler.advanceUntilIdle()
        model.openKitchen()
        testScheduler.advanceUntilIdle()
        val tickets = model.state.value.kds!!.view.tickets
        assertEquals(4, tickets.size, "the bar's, expo's, the kitchen's, expo's")
        model.select(1)
        model.select(5)
        assertEquals(3, model.state.value.kds!!.selected, "kept to the last")
        model.select(-9)
        assertEquals(0, model.state.value.kds!!.selected)
        model.select(tickets[2])
        assertEquals(2, model.state.value.kds!!.selected)
        // The first bumped: the selection stays on the ticket it was on.
        model.bump(tickets[0])
        testScheduler.advanceUntilIdle()
        assertEquals(tickets[2].ticket, model.state.value.kds!!.selectedTicket?.ticket)
    }

    @Test
    fun the_kitchens_reroutes_are_told_and_a_location_without_one_has_no_kitchen_screen() = runTest {
        val till = FakeTill()
        val model = signedIn(till)
        till.alerts += KitchenAlert("ticket1", "Bar", "ORDER1", "Bar's screen didn't show order ORDER1: printed at its printer")
        model.refreshSync()
        testScheduler.advanceUntilIdle()
        assertEquals(1, model.state.value.alerts.size)
        val none = FakeTill().apply { stations = emptyList() }
        val register = model(none)
        register.start()
        testScheduler.advanceUntilIdle()
        register.openKitchen()
        testScheduler.advanceUntilIdle()
        assertEquals("This location has no kitchen.", register.state.value.error)
        assertEquals(Screen.SIGN_IN, register.state.value.screen)
    }

    /** Found in review: a press while the bump's read ran bumped the ticket again, refused. */
    @Test
    fun a_ticket_is_bumped_once_even_while_its_bump_is_read() = runTest {
        val fake = FakeTill()
        var model: RegisterModel? = null
        var pressDuringRead = false
        val till = object : Till by fake {
            override fun kitchenScreen(stations: List<String>): KitchenView {
                if (pressDuringRead) {
                    pressDuringRead = false
                    model!!.bumpSelected()
                }
                return fake.kitchenScreen(stations)
            }
        }
        model = signedIn(till)
        model.tap(model.button("Tea"))
        testScheduler.advanceUntilIdle()
        model.send()
        testScheduler.advanceUntilIdle()
        model.signOut()
        testScheduler.advanceUntilIdle()
        model.openKitchen()
        testScheduler.advanceUntilIdle()
        pressDuringRead = true
        model.bumpSelected()
        testScheduler.advanceUntilIdle()
        assertEquals(1, fake.bumpCalls)
        assertNull(model.state.value.error)
    }

    /** Found in review: a second press of Recall brought back the ticket bumped before it too. */
    @Test
    fun recall_brings_back_one_ticket_however_fast_it_is_pressed() = runTest {
        val till = FakeTill()
        val model = signedIn(till)
        model.tap(model.button("Tea"))
        testScheduler.advanceUntilIdle()
        model.send()
        testScheduler.advanceUntilIdle()
        model.signOut()
        testScheduler.advanceUntilIdle()
        model.openKitchen()
        testScheduler.advanceUntilIdle()
        val both = model.state.value.kds!!.view.tickets
        model.bump(both[0])
        testScheduler.advanceUntilIdle()
        model.bump(both[1])
        testScheduler.advanceUntilIdle()
        assertEquals(emptyList(), model.state.value.kds!!.view.tickets)
        model.recallLast()
        model.recallLast()
        testScheduler.advanceUntilIdle()
        assertEquals(listOf(both[1].ticket), model.state.value.kds!!.view.tickets.map { it.ticket })
    }

    /** Found in review: a publication that signed the cashier out lost the kitchen's stations. */
    @Test
    fun a_publication_keeps_the_kitchen_and_reads_it_again() = runTest {
        val fake = FakeTill()
        var version = "v1"
        var team = fake.team()
        val till = object : Till by fake {
            override fun profileVersion() = version
            override fun team() = team
        }
        val model = signedIn(till)
        assertEquals(3, model.state.value.stations.size)
        // A publication without the cashier: signed out, the kitchen kept.
        team = listOf(Member("sam", "Sam"))
        version = "v2"
        model.refreshSync()
        testScheduler.advanceUntilIdle()
        assertEquals(Screen.SIGN_IN, model.state.value.screen)
        assertEquals(3, model.state.value.stations.size)
        // One that drops the kitchen: read again, and gone.
        fake.stations = emptyList()
        version = "v3"
        model.refreshSync()
        testScheduler.advanceUntilIdle()
        assertEquals(0, model.state.value.stations.size)
    }

    /** Found in review: a sent line's notes could be changed, which the kernel refuses. */
    @Test
    fun a_sent_lines_notes_stay_as_they_were_sent() = runTest {
        val till = FakeTill()
        val model = signedIn(till)
        model.tap(model.button("Tea"))
        testScheduler.advanceUntilIdle()
        model.send()
        testScheduler.advanceUntilIdle()
        model.openNotes(model.state.value.ticket!!.lines[0])
        assertNull(model.state.value.notes)
        assertEquals("tea was sent to the kitchen with its notes.", model.state.value.error)
    }
}
