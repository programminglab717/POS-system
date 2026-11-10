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
    private fun TestScope.signedIn(till: FakeTill = FakeTill()): RegisterModel {
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
        model.choose(milk, milk.modifiers[1])
        model.choose(shots, shots.modifiers[0])
        model.choose(shots, shots.modifiers[0])
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
}
