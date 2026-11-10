package dev.keel.register

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** The register's screens. */
enum class Screen {
    /** Who is working: the team, to sign in. */
    SIGN_IN,

    /** The menu and the order being rung. */
    REGISTER,

    /** Today's orders. */
    ORDERS,

    /** The printer: its address, and its jobs. */
    PRINTER,
}

/** Paying [order] in cash: what it owes, the amounts offered, and an amount the cashier types. */
data class Tender(
    val order: String,
    val due: Amount,
    val offers: List<Amount>,
    /** What the cashier has typed, in minor units, and how it shows. */
    val typed: Amount? = null,
)

/** What the register's screens show. */
data class RegisterState(
    val screen: Screen = Screen.SIGN_IN,
    val loading: Boolean = true,
    val team: List<Member> = emptyList(),
    val member: Member? = null,
    val pages: List<MenuPage> = emptyList(),
    val page: Int = 0,
    val ticket: Ticket? = null,
    /** The modifier sheet open for an item, if any. */
    val sheet: ModifierSheet? = null,
    /** The cash tender open for the order, if any. */
    val tender: Tender? = null,
    /** The change handed back for the last order paid, until the next is begun. */
    val change: String? = null,
    val orders: List<Ticket> = emptyList(),
    /** Where the printer listens, if it is set. */
    val printer: PrinterAddress? = null,
    /** The print jobs, as last read. */
    val jobs: List<PrintJob> = emptyList(),
    /** How replication stands, as last read, for a register joined to its network. */
    val sync: SyncInfo? = null,
    /** The last refusal or failure, until the next intent. */
    val error: String? = null,
)

/**
 * The register's state: who is signed in, the menu, the order being rung, the modifier sheet and
 * the cash tender, and today's orders. Each intent that asks the kernel runs on [worker], one at
 * a time, so the kernel has one caller; the state changes when it answers. A refusal, or any
 * other failure, is kept in the state for the screen to show.
 */
class RegisterModel(
    private val till: Till,
    private val scope: CoroutineScope,
    private val worker: CoroutineDispatcher,
    private val settings: PrinterSettings? = null,
) {
    private val mutable = MutableStateFlow(RegisterState())

    /** What the screens show now. */
    val state: StateFlow<RegisterState> = mutable.asStateFlow()

    private val current get() = mutable.value

    /** Loads the team, for whoever signs in, and where the printer is. */
    fun start() = perform {
        val team = till.team()
        val printer = settings?.load()
        mutable.update { it.copy(loading = false, team = team, printer = printer) }
    }

    /** Signs [member] in, and shows the menu. */
    fun signIn(member: Member) = perform {
        till.signIn(member.id)
        val pages = till.menu()
        mutable.update { it.copy(screen = Screen.REGISTER, member = member, pages = pages, page = 0) }
    }

    /** Signs out: the next cashier signs in. An order being rung stays open, for them. */
    fun signOut() = perform {
        till.signOut()
        mutable.update { RegisterState(loading = false, team = it.team, printer = it.printer) }
    }

    /** Shows the menu's page [index]. */
    fun showPage(index: Int) = mutable.update { state ->
        state.copy(page = index.coerceIn(0, (state.pages.size - 1).coerceAtLeast(0)))
    }

    /** Rings [button]'s item, or opens its modifier sheet if it has modifiers to choose. */
    fun tap(button: MenuButton) = perform {
        if (button.choices) {
            val item = till.item(button.variant)
            mutable.update { it.copy(sheet = ModifierSheet(item)) }
        } else {
            ring(button.variant, emptyList(), 1)
        }
    }

    /** Taps [modifier] in [group] on the open sheet. */
    fun choose(group: Group, modifier: Modifier) = mutable.update { state ->
        state.copy(sheet = state.sheet?.tap(group, modifier), error = null)
    }

    /** Sets how many of the sheet's item to ring. */
    fun sheetQuantity(quantity: Int) = mutable.update { state ->
        state.copy(sheet = state.sheet?.withQuantity(quantity))
    }

    /** Closes the sheet without ringing anything. */
    fun closeSheet() = mutable.update { it.copy(sheet = null) }

    /** Rings the sheet's item with what was chosen, and closes it. */
    fun addFromSheet() = perform {
        val sheet = current.sheet ?: return@perform
        ring(sheet.item.variant, sheet.choices(), sheet.quantity)
        mutable.update { it.copy(sheet = null) }
    }

    /** One more of [line]. */
    fun more(line: TicketLine) = perform {
        val ticket = current.ticket ?: return@perform
        val count = line.count ?: return@perform
        show(till.changeQuantity(ticket.order, line.line, count + 1))
    }

    /** One fewer of [line]: the last one takes the line off. */
    fun fewer(line: TicketLine) = perform {
        val ticket = current.ticket ?: return@perform
        val count = line.count ?: return@perform
        show(
            if (count > 1) till.changeQuantity(ticket.order, line.line, count - 1) else till.removeLine(ticket.order, line.line),
        )
    }

    /** Takes [line] off the order. */
    fun remove(line: TicketLine) = perform {
        val ticket = current.ticket ?: return@perform
        show(till.removeLine(ticket.order, line.line))
    }

    /** Drops the order being rung. */
    fun abandon() = perform {
        val ticket = current.ticket ?: return@perform
        till.abandon(ticket.order)
        mutable.update { it.copy(ticket = null, tender = null) }
    }

    /** Opens the cash tender for the order being rung. */
    fun openTender() = perform {
        val ticket = current.ticket ?: return@perform
        val offers = till.cashOffers(ticket.order)
        val due = offers.firstOrNull() ?: till.amount(ticket.cashDueMinor)
        mutable.update { it.copy(tender = Tender(ticket.order, due, offers)) }
    }

    /** Types [digit] into the tender's amount, in minor units, as a cash keypad does. */
    fun typeDigit(digit: Int) = perform {
        val tender = current.tender ?: return@perform
        val typed = (tender.typed?.minor ?: 0) * 10 + digit.coerceIn(0, 9)
        if (typed > MAX_TYPED) return@perform
        val amount = till.amount(typed)
        mutable.update { state -> state.copy(tender = state.tender?.copy(typed = amount)) }
    }

    /** Takes the last digit typed off the tender's amount. */
    fun eraseDigit() = perform {
        val tender = current.tender ?: return@perform
        val typed = (tender.typed?.minor ?: 0) / 10
        val amount = if (typed == 0L) null else till.amount(typed)
        mutable.update { state -> state.copy(tender = state.tender?.copy(typed = amount)) }
    }

    /** Closes the tender without paying. */
    fun closeTender() = mutable.update { it.copy(tender = null) }

    /** Pays the order with [tendered], and shows the change. */
    fun pay(tendered: Amount) = perform {
        val tender = current.tender ?: return@perform
        val paid = till.payCash(tender.order, tendered.minor)
        mutable.update { it.copy(ticket = null, tender = null, change = paid.change) }
    }

    /** Shows today's orders. */
    fun showOrders() = perform {
        val orders = till.ordersToday()
        mutable.update { it.copy(screen = Screen.ORDERS, orders = orders) }
    }

    /** Takes up [order] again, if it is open, on the register. */
    fun resume(order: Ticket) = perform {
        val ticket = till.ticket(order.order)
        if (ticket.state != OrderState.OPEN) throw TillException("That order isn't open.")
        mutable.update { it.copy(screen = Screen.REGISTER, ticket = ticket, change = null) }
    }

    /** Prints the closed [order]'s receipt again. */
    fun reprint(order: Ticket) = perform {
        if (order.state != OrderState.CLOSED) throw TillException("Only a paid order has a receipt.")
        till.reprint(order.order)
        val jobs = till.printJobs()
        mutable.update { it.copy(jobs = jobs) }
    }

    /** Shows the printer: its address, and its jobs. */
    fun showPrinter() = perform {
        val jobs = till.printJobs()
        mutable.update { it.copy(screen = Screen.PRINTER, jobs = jobs) }
    }

    /** Reads the print jobs again. */
    fun refreshJobs() = perform {
        val jobs = till.printJobs()
        mutable.update { it.copy(jobs = jobs) }
    }

    /** Keeps where the printer listens: [host], and [port] if it is one. */
    fun setPrinter(host: String, port: String) = perform {
        val trimmed = host.trim()
        val number = port.trim().toIntOrNull()
        if (trimmed.isEmpty() || trimmed.any { it.isWhitespace() }) throw TillException("A printer's address is a name or an IP address.")
        if (number == null || number !in 1..65_535) throw TillException("A port is a number from 1 to 65535.")
        val address = PrinterAddress(trimmed, number)
        settings?.save(address)
        mutable.update { it.copy(printer = address) }
    }

    /** What staff decide about the unconfirmed [job]: that it [printed], or to print it again. */
    fun resolve(job: PrintJob, printed: Boolean) = perform {
        till.resolve(job.job, printed)
        val jobs = till.printJobs()
        mutable.update { it.copy(jobs = jobs) }
    }

    /** Reads how replication stands again; the shell calls it every few seconds. */
    fun refreshSync() = scope.launch {
        // A status that can't be read is no reason to show the cashier an error.
        val sync = runCatching { withContext(worker) { till.syncState() } }.getOrNull()
        mutable.update { it.copy(sync = sync) }
    }

    /** Back to the register from today's orders or the printer. */
    fun showRegister() = mutable.update { it.copy(screen = Screen.REGISTER) }

    /** Rings [variant], on a new order if none is open. */
    private fun ring(variant: String, choices: List<Chosen>, quantity: Int) {
        // An order begun is shown at once, so a line it refuses doesn't leave it behind.
        val order = current.ticket?.order ?: till.startOrder().also(::show).order
        show(till.addItem(order, variant, choices, quantity))
    }

    private fun show(ticket: Ticket) = mutable.update { it.copy(ticket = ticket, change = null) }

    /** Runs [intent] on the worker, clearing the last refusal first and keeping a new one. */
    private fun perform(intent: () -> Unit) {
        mutable.update { it.copy(error = null) }
        scope.launch {
            try {
                withContext(worker) { intent() }
            } catch (refused: TillException) {
                mutable.update { it.copy(loading = false, error = refused.message) }
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (failed: Exception) {
                // Anything else the kernel or the platform throws is shown, not a crash: the
                // register stays up for whoever can look.
                mutable.update {
                    val what = failed.message?.takeIf { message -> message.isNotBlank() } ?: failed::class.simpleName
                    it.copy(loading = false, error = "Something went wrong: $what")
                }
            }
        }
    }

    private companion object {
        /** The most a cashier may type, in minor units: ten digits. */
        const val MAX_TYPED = 9_999_999_999L
    }
}
