package dev.keel.register

import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** The register's card payments (ADR-0035), against the fake till, which a test answers as the terminal would. */
class CardModelTest {
    private fun TestScope.signedIn(till: FakeTill, terminal: CardTerminal? = SimulatedTerminal()): RegisterModel {
        val model = RegisterModel(till, this, StandardTestDispatcher(testScheduler), terminal = terminal)
        model.start()
        testScheduler.advanceUntilIdle()
        model.signIn(model.state.value.team.first())
        testScheduler.advanceUntilIdle()
        return model
    }

    /** A tea, $3.00, rung. */
    private fun TestScope.aTea(model: RegisterModel) {
        model.tap(model.state.value.pages.flatMap { it.buttons }.single { it.name == "Tea" })
        testScheduler.advanceUntilIdle()
    }

    @Test
    fun a_tip_chosen_on_the_screen_goes_with_the_sale_and_the_approval_closes_the_order() = runTest {
        val till = FakeTill()
        val model = signedIn(till)
        aTea(model)
        model.openCard()
        testScheduler.advanceUntilIdle()
        val sheet = assertNotNull(model.state.value.card)
        assertTrue(sheet.asksTip, "the location asks on the screen")
        // No tip, each preset as a percentage and an amount, and a custom one, alike (H-10).
        assertEquals(listOf(1_500 to "$0.45", 2_000 to "$0.60"), sheet.offers.presets.map { it.percent to it.amount.text })
        model.chooseTip(CardTip.Amount(45))
        testScheduler.advanceUntilIdle()
        val waiting = assertNotNull(model.state.value.card?.payment)
        assertEquals(CardState.Waiting, waiting.state)
        assertEquals(listOf<CardTip>(CardTip.Amount(45)), till.cardTips)
        till.answerCard(waiting.payment, approvedMinor = 345)
        model.refreshCard()
        testScheduler.advanceUntilIdle()
        assertNull(model.state.value.card, "paid: the sheet closes")
        assertNull(model.state.value.ticket)
    }

    @Test
    fun a_custom_tip_is_typed_as_a_keypad_types_it() = runTest {
        val till = FakeTill()
        val model = signedIn(till)
        aTea(model)
        model.openCard()
        testScheduler.advanceUntilIdle()
        model.customTip()
        listOf(1, 2, 5).forEach(model::typeTipDigit)
        testScheduler.advanceUntilIdle()
        assertEquals("$1.25", model.state.value.card?.typed?.text)
        model.eraseTipDigit()
        testScheduler.advanceUntilIdle()
        assertEquals("$0.12", model.state.value.card?.typed?.text)
    }

    @Test
    fun a_partial_approval_leaves_the_rest_due_and_another_card_pays_it() = runTest {
        val till = FakeTill()
        val model = signedIn(till)
        aTea(model)
        model.openCard()
        testScheduler.advanceUntilIdle()
        model.chooseTip(CardTip.None)
        testScheduler.advanceUntilIdle()
        val first = model.state.value.card!!.payment!!
        till.answerCard(first.payment, approvedMinor = 100)
        model.refreshCard()
        testScheduler.advanceUntilIdle()
        val approved = assertIs<CardState.Approved>(model.state.value.card?.payment?.state)
        assertEquals("$1.00", approved.applied.text)
        assertEquals(200, model.state.value.ticket?.cashDueMinor, "the rest is due")
        model.cardAgain()
        testScheduler.advanceUntilIdle()
        model.chooseTip(CardTip.None)
        testScheduler.advanceUntilIdle()
        val second = model.state.value.card!!.payment!!
        assertEquals("$2.00", second.amount.text)
        till.answerCard(second.payment, approvedMinor = 200)
        model.refreshCard()
        testScheduler.advanceUntilIdle()
        assertNull(model.state.value.ticket)
    }

    @Test
    fun a_decline_shows_why_and_can_be_tried_again() = runTest {
        val till = FakeTill()
        val model = signedIn(till)
        aTea(model)
        model.openCard()
        testScheduler.advanceUntilIdle()
        model.chooseTip(CardTip.None)
        testScheduler.advanceUntilIdle()
        val payment = model.state.value.card!!.payment!!
        till.answerCard(payment.payment, state = CardState.Declined("insufficient_funds"))
        model.refreshCard()
        testScheduler.advanceUntilIdle()
        assertEquals(CardState.Declined("insufficient_funds"), model.state.value.card?.payment?.state)
        assertEquals(300, model.state.value.ticket?.cashDueMinor)
        model.cardAgain()
        testScheduler.advanceUntilIdle()
        assertNull(model.state.value.card?.payment, "a new payment, its tip offered again")
    }

    @Test
    fun a_payment_no_terminal_answers_for_is_settled_by_a_manager() = runTest {
        val till = FakeTill()
        val model = signedIn(till)
        aTea(model)
        model.openCard()
        testScheduler.advanceUntilIdle()
        model.chooseTip(CardTip.Amount(45))
        testScheduler.advanceUntilIdle()
        val payment = model.state.value.card!!.payment!!
        till.answerCard(payment.payment, state = CardState.Checking)
        model.refreshCard()
        testScheduler.advanceUntilIdle()
        assertEquals(CardState.Checking, model.state.value.card?.payment?.state)
        // The order takes no other payment while it is checked.
        model.openTender()
        testScheduler.advanceUntilIdle()
        model.pay(model.state.value.tender!!.offers.first())
        testScheduler.advanceUntilIdle()
        model.settleCard(charged = true)
        testScheduler.advanceUntilIdle()
        assertEquals("payment.resolve", model.state.value.approval?.action)
        model.chooseApprover(Member("sam", "Sam"))
        till.approvedBy = null
        model.approve("")
        testScheduler.advanceUntilIdle()
        assertNull(model.state.value.ticket, "charged, by hand: the order is paid")
    }

    @Test
    fun the_terminal_asks_for_the_tip_where_the_location_says_and_it_can() = runTest {
        val till = FakeTill().apply { tipping = TipPrompt.TERMINAL }
        val model = signedIn(till)
        aTea(model)
        model.openCard()
        testScheduler.advanceUntilIdle()
        assertNotNull(model.state.value.card?.payment, "straight to the terminal")
        assertEquals(listOf<CardTip>(CardTip.Terminal), till.cardTips)
        // A terminal that can't ask leaves it to the screen.
        val other = FakeTill().apply { tipping = TipPrompt.TERMINAL }
        val asking = signedIn(other, SimulatedTerminal(asksTip = false))
        aTea(asking)
        asking.openCard()
        testScheduler.advanceUntilIdle()
        assertEquals(true, asking.state.value.card?.asksTip)
        // With no tipping, the sale goes at once, with no tip.
        val none = FakeTill().apply { tipping = null }
        val plain = signedIn(none)
        aTea(plain)
        plain.openCard()
        testScheduler.advanceUntilIdle()
        assertEquals(listOf<CardTip>(CardTip.None), none.cardTips)
    }

    @Test
    fun a_register_with_no_terminal_takes_no_cards() = runTest {
        val model = signedIn(FakeTill(), terminal = null)
        aTea(model)
        model.openCard()
        testScheduler.advanceUntilIdle()
        assertNull(model.state.value.card)
        assertEquals("This register has no card terminal.", model.state.value.error)
    }
}

/** What the Android review found (ADR-0035). */
class CardReviewTest {
    private fun TestScope.signedIn(till: FakeTill): RegisterModel {
        val model = RegisterModel(till, this, StandardTestDispatcher(testScheduler), terminal = SimulatedTerminal())
        model.start()
        testScheduler.advanceUntilIdle()
        model.signIn(model.state.value.team.first())
        testScheduler.advanceUntilIdle()
        model.tap(model.state.value.pages.flatMap { it.buttons }.single { it.name == "Tea" })
        testScheduler.advanceUntilIdle()
        return model
    }

    /** A payment closed while checked is opened again by Card, where a manager can settle it; its ticket follows it meanwhile. */
    @Test
    fun a_payment_whose_sheet_closed_is_opened_again_never_started_anew() = runTest {
        val till = FakeTill()
        val model = signedIn(till)
        model.openCard()
        testScheduler.advanceUntilIdle()
        model.chooseTip(CardTip.None)
        testScheduler.advanceUntilIdle()
        val payment = model.state.value.card!!.payment!!
        till.answerCard(payment.payment, state = CardState.Checking)
        model.closeCard()
        assertEquals(model.state.value.ticket?.order, model.state.value.watching)
        model.openCard()
        testScheduler.advanceUntilIdle()
        assertEquals(payment.payment, model.state.value.card?.payment?.payment, "the same payment")
        assertEquals(CardState.Checking, model.state.value.card?.payment?.state)
        assertEquals(1, till.cardPays.size, "none started anew")
        // Closed again, an approval meanwhile closes the order on the ticket.
        model.closeCard()
        till.answerCard(payment.payment, approvedMinor = 300)
        model.refreshCard()
        testScheduler.advanceUntilIdle()
        assertNull(model.state.value.ticket)
        assertNull(model.state.value.watching)
    }

    @Test
    fun a_second_tap_on_card_starts_nothing_more() = runTest {
        val till = FakeTill().apply { tipping = null }
        val model = signedIn(till)
        model.openCard()
        model.openCard()
        testScheduler.advanceUntilIdle()
        assertEquals(1, till.cardPays.size)
        assertNotNull(model.state.value.card?.payment)
        assertNull(model.state.value.error)
    }

    /** A refresh leaves the error a cashier is reading. */
    @Test
    fun a_refresh_keeps_the_error_shown() = runTest {
        val till = FakeTill()
        val model = signedIn(till)
        model.openCard()
        testScheduler.advanceUntilIdle()
        model.chooseTip(CardTip.Amount(5_000))
        testScheduler.advanceUntilIdle()
        val error = model.state.value.error
        assertNotNull(error, "a tip beyond the payment is refused")
        model.refreshCard()
        model.refreshOwedRefunds()
        testScheduler.advanceUntilIdle()
        assertEquals(error, model.state.value.error)
    }

    @Test
    fun a_tip_of_nothing_is_no_tip() = runTest {
        val model = signedIn(FakeTill())
        model.openCard()
        testScheduler.advanceUntilIdle()
        model.customTip()
        model.typeTipDigit(0)
        testScheduler.advanceUntilIdle()
        assertNull(model.state.value.card?.typed)
    }

    /** The cashier chooses where a refund goes: cash or a card. */
    @Test
    fun a_refund_goes_where_the_cashier_says() = runTest {
        val model = signedIn(FakeTill())
        model.refundTo("card:x")
        assertNull(model.state.value.returning, "no return open: nothing changes")
    }
}

/** The simulated terminal's waits and cancels: each for its own request. */
class SimulatedTerminalWaitTest {
    @Test
    fun a_cancel_ends_only_its_own_sale_even_before_it_is_taken() = runTest {
        val terminal = SimulatedTerminal()
        terminal.cancel("p1")
        val sale = TerminalRequest("p1", TerminalRequest.Kind.SALE, amountMinor = 300)
        assertEquals(TerminalOutcome.Declined("cancelled"), terminal.take(sale))
        // A refund needs no customer: approved at once.
        val refund = TerminalRequest("r1-1", TerminalRequest.Kind.REFUND, amountMinor = 300, reference = "pi_1")
        assertIs<TerminalOutcome.Approved>(terminal.take(refund))
    }

    @Test
    fun the_terminal_asked_for_the_tip_adds_its_first_preset() = runTest {
        val terminal = SimulatedTerminal(auto = SimulatedAnswer.APPROVE)
        val sale = TerminalRequest("p1", TerminalRequest.Kind.SALE, amountMinor = 300, presets = listOf(1_500, 2_000))
        assertEquals(345, assertIs<TerminalOutcome.Approved>(terminal.take(sale)).approvedMinor)
    }
}

/** The simulated terminal: one request at a time, each performed once, and asked about truthfully. */
class SimulatedTerminalTest {
    @Test
    fun it_never_charges_a_key_twice_and_answers_questions_from_its_ledger() = runTest {
        val terminal = SimulatedTerminal(auto = SimulatedAnswer.NO_ANSWER)
        val sale = TerminalRequest("p1", TerminalRequest.Kind.SALE, amountMinor = 300, tipMinor = 45, partial = true)
        assertEquals(TerminalOutcome.Unknown, terminal.take(sale))
        // Asked about it, it says what it did: charged, the tip included.
        val asked = assertIs<TerminalOutcome.Approved>(terminal.take(sale.copy(status = true)))
        assertEquals(345, asked.approvedMinor)
        // Asked to perform it again, it answers as before, charging nothing more.
        assertEquals(asked, terminal.take(sale))
        assertEquals(TerminalOutcome.NotFound, terminal.take(sale.copy(key = "p2", status = true)))
    }

    @Test
    fun a_partial_approval_is_half_and_only_where_allowed() = runTest {
        val terminal = SimulatedTerminal(auto = SimulatedAnswer.APPROVE_PART)
        val sale = TerminalRequest("p1", TerminalRequest.Kind.SALE, amountMinor = 300, partial = true)
        assertEquals(150, assertIs<TerminalOutcome.Approved>(terminal.take(sale)).approvedMinor)
        assertIs<TerminalOutcome.Declined>(terminal.take(sale.copy(key = "p2", partial = false)))
    }
}

/** The stock screen (ADR-0036): items tracked from the menu, received, adjusted on a manager's word, counted. */
class StockModelTest {
    private fun TestScope.signedIn(till: FakeTill): RegisterModel {
        val model = RegisterModel(till, this, StandardTestDispatcher(testScheduler))
        model.start()
        testScheduler.advanceUntilIdle()
        model.signIn(model.state.value.team.first())
        testScheduler.advanceUntilIdle()
        return model
    }

    @Test
    fun an_item_chosen_from_the_menu_is_received_adjusted_and_counted() = runTest {
        val till = FakeTill().apply { stockNeedsApproval = true }
        val model = signedIn(till)
        model.openStock()
        testScheduler.advanceUntilIdle()
        assertEquals(Screen.STOCK, model.state.value.screen)
        assertEquals(emptyList(), model.state.value.stock?.items)
        model.chooseStockItem()
        val tea = model.state.value.pages.flatMap { it.buttons }.single { it.name == "Tea" }
        model.trackStock(tea)
        assertEquals(tea.variant, model.state.value.stock?.selected?.variant)
        // Received: 12.
        model.startStockEntry(StockMovement.RECEIVED)
        model.typeStockDigit(1)
        model.typeStockDigit(2)
        model.confirmStockEntry()
        testScheduler.advanceUntilIdle()
        assertEquals("12", model.state.value.stock?.selected?.onHand?.text)
        assertNull(model.state.value.stock?.entry)
        // Taken off by 2, which needs a manager.
        model.startStockEntry(StockMovement.ADJUSTED)
        model.stockDown(true)
        model.typeStockDigit(2)
        model.confirmStockEntry()
        testScheduler.advanceUntilIdle()
        assertEquals("stock.adjust", model.state.value.approval?.action)
        model.chooseApprover(Member("sam", "Sam"))
        till.approvedBy = "sam"
        model.approve("")
        testScheduler.advanceUntilIdle()
        assertEquals("10", model.state.value.stock?.selected?.onHand?.text)
        // A movement of nothing is refused at once; a count of nothing is a count.
        model.startStockEntry(StockMovement.RECEIVED)
        model.confirmStockEntry()
        assertEquals("Type how many.", model.state.value.error)
        model.cancelStockEntry()
        till.stockNeedsApproval = false
        model.startStockEntry(null)
        model.confirmStockEntry()
        testScheduler.advanceUntilIdle()
        assertEquals("0", model.state.value.stock?.selected?.onHand?.text)
        assertEquals(listOf<StockMovement?>(StockMovement.RECEIVED, StockMovement.ADJUSTED, null), till.stockMoves.map { it.first })
    }

    @Test
    fun a_return_can_be_kept_out_of_stock() = runTest {
        val till = FakeTill()
        val model = signedIn(till)
        model.tap(model.state.value.pages.flatMap { it.buttons }.single { it.name == "Tea" })
        testScheduler.advanceUntilIdle()
        model.openTender()
        testScheduler.advanceUntilIdle()
        model.pay(model.state.value.tender!!.offers[0])
        testScheduler.advanceUntilIdle()
        model.beginReturn(till.ticket("order1"))
        testScheduler.advanceUntilIdle()
        val sheet = model.state.value.returning!!
        assertTrue(sheet.restock, "back to stock unless the cashier says")
        model.returnToStock(false)
        model.pickMore(sheet.returnable.lines.single())
        model.returnReason("damaged")
        model.confirmReturn()
        testScheduler.advanceUntilIdle()
        assertEquals(false, till.lastRestock)
    }
}
