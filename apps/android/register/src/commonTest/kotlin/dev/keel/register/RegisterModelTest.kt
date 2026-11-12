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
        assertEquals(false, model.state.value.askDrawer)
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
        assertEquals("$3.00", model.state.value.drawer?.taken)
        assertNull(model.state.value.drawer?.expected, "the close is blind")
        // $5.00 dropped, then a blind count of $97.50: $0.50 short.
        model.beginCash(CashPurpose.DROP)
        model.cashReason("safe_drop")
        for (digit in listOf(5, 0, 0)) model.typeCashDigit(digit)
        testScheduler.advanceUntilIdle()
        model.confirmCash()
        testScheduler.advanceUntilIdle()
        assertEquals("$5.00", model.state.value.drawer?.dropped)
        model.beginCash(CashPurpose.COUNT)
        for (digit in listOf(9, 7, 5, 0)) model.typeCashDigit(digit)
        testScheduler.advanceUntilIdle()
        model.confirmCash()
        testScheduler.advanceUntilIdle()
        assertNull(model.state.value.drawer)
        val closed = model.state.value.closedDrawer!!
        assertEquals(Triple("$98.00", "$97.50", "-$0.50"), Triple(closed.expected, closed.counted, closed.variance))
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
        assertEquals("$2.00", model.state.value.drawer?.paidOut)
        // The Z waits for the drawer to close.
        model.showReport(z = true)
        testScheduler.advanceUntilIdle()
        assertEquals("Close the drawer first.", model.state.value.error)
    }
}
