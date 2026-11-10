package dev.keel.register

/**
 * What the register asks of the kernel, in the register's own words: the screens know the
 * kernel only through this, so their state can be tested against a fake (ADR-0027).
 *
 * Every call may block, on the store's disk or its lock: callers make them off the UI's thread.
 * A refusal the cashier should see is a [TillException].
 */
interface Till {
    /** The team members who may sign in, in the profile's order. */
    fun team(): List<Member>

    /** Signs [member] in. */
    fun signIn(member: String)

    /** Signs whoever is signed in out. */
    fun signOut()

    /** The menu's pages. */
    fun menu(): List<MenuPage>

    /** [variant], with the modifier groups offered for it. */
    fun item(variant: String): Item

    /** Starts an order, and gives its ticket. */
    fun startOrder(): Ticket

    /** Rings [quantity] of [variant] with [choices] on [order], and gives the ticket. */
    fun addItem(order: String, variant: String, choices: List<Chosen>, quantity: Int): Ticket

    /** Changes how many of [line] [order] has, and gives the ticket. */
    fun changeQuantity(order: String, line: String, quantity: Int): Ticket

    /** Takes [line] off [order], and gives the ticket. */
    fun removeLine(order: String, line: String): Ticket

    /** Drops [order], and gives its ticket. */
    fun abandon(order: String): Ticket

    /** What the register offers as cash tendered for [order]: what it owes, then round amounts. */
    fun cashOffers(order: String): List<Amount>

    /** [minor] of the location's currency, as the register shows it. */
    fun amount(minor: Long): Amount

    /** Pays [order] in cash, [tenderedMinor] in the location's minor units. */
    fun payCash(order: String, tenderedMinor: Long): Paid

    /** [order]'s ticket. */
    fun ticket(order: String): Ticket

    /** The orders begun today, whatever became of them, oldest first. */
    fun ordersToday(): List<Ticket>

    /** The print jobs queued, with the printer, and unconfirmed. */
    fun printJobs(): List<PrintJob>

    /**
     * What staff decide about the unconfirmed [job]: that it [printed], or that it is printed
     * again.
     */
    fun resolve(job: String, printed: Boolean)

    /** Prints the receipt of the closed [order] again, marked as a reprint. */
    fun reprint(order: String)

    /** How replication with the location stands, for a register joined to its network. */
    fun syncState(): SyncInfo? = null
}

/** How replication with the location stands. */
data class SyncInfo(
    /** This register listens for the location's devices, as its hub's address. */
    val listening: Boolean,
    /** Its log is settled: until it is, it doesn't write. */
    val settled: Boolean,
    /** A hub is reachable. */
    val hubReachable: Boolean,
    /** This register serves as the hub now. */
    val serving: Boolean,
    /**
     * The version of the profile the register rings from: when the cloud publishes another, it
     * changes, and the team and the menu are read again.
     */
    val profileVersion: String = "",
)

/** What a print job prints. */
enum class JobKind { RECEIPT, DRAWER }

/** Where a print job is. */
enum class JobState {
    /** Waiting for the printer, or to be tried again. */
    QUEUED,

    /** With the printer. */
    SENDING,

    /** Some of it may have been sent: staff decide whether it printed. */
    UNCONFIRMED,
}

/** A print job, as the register shows it. */
data class PrintJob(val job: String, val kind: JobKind, val state: JobState, val attempts: Int)

/** Where the receipt printer listens on the store's network. */
data class PrinterAddress(val host: String, val port: Int = 9100)

/** Where the printer's address is kept, on the device. */
interface PrinterSettings {
    /** The address kept, if one is. */
    fun load(): PrinterAddress?

    /** Keeps [address]. */
    fun save(address: PrinterAddress)
}

/** A team member who may sign in. */
data class Member(val id: String, val name: String)

/** An amount in minor units, and how the locale shows it. */
data class Amount(val minor: Long, val text: String)

/** A page of the menu. */
data class MenuPage(val name: String, val buttons: List<MenuButton>)

/**
 * A button of the menu: a variant to ring, its name and price as the locale shows them, and
 * whether it has modifiers to choose, and whether some must be.
 */
data class MenuButton(
    val variant: String,
    val name: String,
    val price: String,
    val choices: Boolean = false,
    val required: Boolean = false,
)

/** An item's variant and the modifier groups offered for it, as the modifier sheet shows them. */
data class Item(val variant: String, val name: String, val price: String, val groups: List<Group>)

/**
 * A modifier group: offered by the item, or under one of its modifiers ([under]), each followed in
 * [Item.groups] by the groups under its own modifiers.
 */
data class Group(
    val group: String,
    val under: String?,
    val name: String,
    val min: Int,
    val max: Int,
    val free: Int,
    val repeat: Boolean,
    val modifiers: List<Modifier>,
    /** How deep it is: 1 for a group the item offers, 2 for one under its modifiers, and so on. */
    val depth: Int = if (under == null) 1 else 2,
)

/** A modifier a group offers, and the price of one application. */
data class Modifier(val modifier: String, val name: String, val price: String)

/**
 * A cashier's choice of a modifier, flattened as the kernel takes them: a choice under a modifier
 * names the index of that modifier's choice, earlier in the list, as its [parent].
 */
data class Chosen(val modifier: String, val quantity: Int, val parent: Int? = null)

/** Where an order is in its life. */
enum class OrderState { OPEN, CLOSED, VOIDED, ABANDONED }

/** An order as the register shows it. */
data class Ticket(
    val order: String,
    val lines: List<TicketLine>,
    val total: String,
    /** What it still owes in cash, in minor units, rounded as the location rounds cash. */
    val cashDueMinor: Long,
    val cashDue: String,
    val state: OrderState = OrderState.OPEN,
    val subtotal: String = total,
    val tax: String? = null,
)

/** A line of a ticket: what it sells, how many, its modifiers in words, and its amount. */
data class TicketLine(
    val line: String,
    val name: String,
    val quantity: String,
    val amount: String,
    /** How many, if it is a whole number; a line sold by weight has none. */
    val count: Int? = null,
    val modifiers: List<String> = emptyList(),
)

/** What paying cash did: the change to hand back, and the ticket afterwards. */
data class Paid(val change: String, val ticket: Ticket)

/** A refusal the cashier sees, in words for them. */
class TillException(message: String, cause: Throwable? = null) : Exception(message, cause)
