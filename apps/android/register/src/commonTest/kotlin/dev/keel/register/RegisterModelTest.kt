package dev.keel.register

import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

/** A till with two buttons, $4.50 and $3.00, whose orders owe what was rung. */
private class FakeTill : Till {
    var signedIn: String? = null
    private val prices = mapOf("latte" to 450L, "tea" to 300L)
    private val lines = mutableListOf<TicketLine>()
    private var orders = 0

    override fun signIn(member: String) {
        if (member != "alex") throw TillException("no such member")
        signedIn = member
    }

    override fun menu() = listOf(
        MenuPage("Drinks", listOf(MenuButton("latte", "Latte", "$4.50"), MenuButton("tea", "Tea", "$3.00"))),
    )

    override fun startOrder(): Ticket {
        orders += 1
        lines.clear()
        return ticket()
    }

    override fun addItem(order: String, variant: String): Ticket {
        val price = prices[variant] ?: throw TillException("not on the menu")
        lines += TicketLine("line${lines.size}", variant, "1", money(price))
        return ticket()
    }

    override fun payCash(order: String, tenderedMinor: Long): Paid {
        val due = due()
        if (tenderedMinor < due) throw TillException("short")
        return Paid(money(tenderedMinor - due), ticket())
    }

    private fun due() = lines.sumOf { line -> prices.getValue(line.name) }

    private fun ticket() = Ticket("order$orders", lines.toList(), money(due()), due(), money(due()))

    private fun money(minor: Long) = "$" + minor / 100 + "." + (minor % 100).toString().padStart(2, '0')
}

class RegisterModelTest {
    private fun TestScope.model(till: Till) =
        RegisterModel(till, this, StandardTestDispatcher(testScheduler))

    @Test
    fun a_cashier_signs_in_rings_and_pays() = runTest {
        val till = FakeTill()
        val model = model(till)
        assertEquals(true, model.state.value.loading)
        model.start("alex")
        testScheduler.advanceUntilIdle()
        assertEquals("alex", till.signedIn)
        assertEquals(listOf("Latte", "Tea"), model.state.value.pages.single().buttons.map { it.name })
        // The first tap starts an order; the next rings on it.
        model.tap("latte")
        testScheduler.advanceUntilIdle()
        model.tap("tea")
        testScheduler.advanceUntilIdle()
        val ticket = model.state.value.ticket!!
        assertEquals(listOf("latte", "tea"), ticket.lines.map { it.name })
        assertEquals("$7.50", ticket.total)
        model.payExact()
        testScheduler.advanceUntilIdle()
        assertNull(model.state.value.ticket)
        assertEquals("$0.00", model.state.value.change)
    }

    @Test
    fun a_refusal_is_shown_until_the_next_intent() = runTest {
        val model = model(FakeTill())
        model.start("sam")
        testScheduler.advanceUntilIdle()
        assertEquals("no such member", model.state.value.error)
        model.start("alex")
        testScheduler.advanceUntilIdle()
        assertNull(model.state.value.error)
        model.tap("scone")
        testScheduler.advanceUntilIdle()
        assertEquals("not on the menu", model.state.value.error)
    }
}
