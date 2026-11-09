package dev.keel.register

/**
 * What the register asks of the kernel, in the register's own words: the screens know the
 * kernel only through this, so their state can be tested against a fake (ADR-0027).
 *
 * Every call may block, on the store's disk or its lock: callers make them off the UI's thread.
 */
interface Till {
    /** Signs [member] in. */
    fun signIn(member: String)

    /** The menu's pages. */
    fun menu(): List<MenuPage>

    /** Starts an order, and gives its ticket. */
    fun startOrder(): Ticket

    /** Rings one of [variant] on [order], and gives the ticket. */
    fun addItem(order: String, variant: String): Ticket

    /** Pays [order] in cash, [tenderedMinor] in the location's minor units. */
    fun payCash(order: String, tenderedMinor: Long): Paid
}

/** A page of the menu. */
data class MenuPage(val name: String, val buttons: List<MenuButton>)

/** A button of the menu: a variant to ring, its name and price as the locale shows them. */
data class MenuButton(val variant: String, val name: String, val price: String)

/** An order as the register shows it. */
data class Ticket(
    val order: String,
    val lines: List<TicketLine>,
    val total: String,
    /** What it still owes in cash, in minor units, rounded as the location rounds cash. */
    val cashDueMinor: Long,
    val cashDue: String,
)

/** A line of a ticket. */
data class TicketLine(val line: String, val name: String, val quantity: String, val amount: String)

/** What paying cash did: the change to hand back, and the ticket afterwards. */
data class Paid(val change: String, val ticket: Ticket)

/** A refusal the cashier sees, in words for them. */
class TillException(message: String, cause: Throwable? = null) : Exception(message, cause)
