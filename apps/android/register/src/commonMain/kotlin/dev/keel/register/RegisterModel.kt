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

    /** The drawer: its movements, its close, and the day's reports (ADR-0031). */
    DRAWER,

    /** A return: a paid order's lines, what of each to return, and its refund (ADR-0032). */
    RETURN,

    /** The kitchen screen: a station's tickets, to bump (ADR-0034). No one signs in. */
    KITCHEN,
}

/** Where a kitchen screen's stations are kept, on the device: which it shows is its own setting. */
interface KitchenSettings {
    /** The stations kept, if some are. */
    fun load(): Set<String>?

    /** Keeps [stations]. */
    fun save(stations: Set<String>)
}

/**
 * The kitchen screen (ADR-0034): the stations it shows, what it shows of them, the ticket
 * selected, as a bump bar's keys move it, and the tickets being bumped, so a second press
 * bumps nothing more.
 */
data class KdsState(
    val stations: Set<String>,
    val view: KitchenView = KitchenView(),
    val selected: Int = 0,
    /** Whether the stations are being chosen. */
    val choosing: Boolean = false,
    val bumping: Set<String> = emptySet(),
) {
    /** The ticket selected, if any is shown. */
    val selectedTicket: KdsTicket? get() = view.tickets.getOrNull(selected)
}

/**
 * A return being made (ADR-0032): the order's lines and what is left of each, how many of each
 * are picked to return, and why. A line sold by measure is picked whole: its count is null.
 */
data class ReturnSheet(
    val returnable: Returnable,
    val picks: Map<String, Long?> = emptyMap(),
    /** Why: none until the cashier says. Found in review: every return was "damaged". */
    val reason: String? = null,
    /** Whether the refund is being made: nothing more is picked or refunded until it is done. */
    val refunding: Boolean = false,
)

/**
 * What the drawer screen is typing an amount for: a float to open it, a movement, or the blind
 * count to close it, in minor units, as a cash keypad types it.
 */
data class CashEntry(
    val purpose: CashPurpose,
    val reason: String = purpose.reason,
    val typed: Amount? = null,
)

/**
 * What an amount typed on the drawer screen is for, or a no-sale, which has a reason and no
 * amount; with the reason each starts with. Found in review: a drop nobody touched was recorded
 * as change.
 */
enum class CashPurpose(val reason: String) {
    FLOAT("change"),
    PAID_IN("change"),
    PAID_OUT("supplier"),
    DROP("safe_drop"),
    COUNT("change"),
    NO_SALE("change_for_customer"),
    ;

    /** Whether it moves cash, which must be more than nothing. */
    val moves: Boolean get() = this == PAID_IN || this == PAID_OUT || this == DROP
}

/**
 * Paying [order] in cash, or one [check] of it when it is split: what it owes, the amounts
 * offered, and an amount the cashier types, which is tendered for all of it, or paid as a part.
 */
data class Tender(
    val order: String,
    val due: Amount,
    val offers: List<Amount>,
    /** What the cashier has typed, in minor units, and how it shows. */
    val typed: Amount? = null,
    /** The check paid, for a split order; null for the whole order. */
    val check: TicketCheck? = null,
)

/**
 * The discount sheet: a discount for [line], or for the whole order when it is null, for [reason],
 * a percentage tapped or an amount typed, in minor units, as a cash keypad types it (ADR-0030).
 */
data class DiscountSheet(
    val line: TicketLine? = null,
    val reason: String = DISCOUNT_REASONS.first().code,
    val typed: Amount? = null,
) {
    /** The percentages offered, in basis points: 5%, 10%, 15%, 20%, 50% and all of it. */
    val percents: List<Int> get() = PERCENTS

    private companion object {
        val PERCENTS = listOf(500, 1_000, 1_500, 2_000, 5_000, 10_000)
    }
}

/** The price sheet: a new unit price for [line], typed as a cash keypad types it, for [reason] (ADR-0030). */
data class PriceSheet(
    val line: TicketLine,
    val reason: String = PRICE_REASONS.first().code,
    val typed: Amount? = null,
)

/** The notes sheet: [line]'s notes, as they are when it opens. */
data class NotesSheet(val line: TicketLine)

/**
 * The 86 sheet for a menu button (ADR-0033): its variant, and the modifiers its item offers, each
 * 86'd for the day or until put back, or put back; [working] while a change is being made.
 */
data class MarkSheet(
    val button: MenuButton,
    val modifiers: List<Modifier> = emptyList(),
    val working: Boolean = false,
)

/** A manager's approval the register asks for: the action, why, and the manager asked, once chosen. */
data class Approval(
    val action: String,
    val reason: String,
    /** Who may approve it, as the kernel says: never the cashier. */
    val approvers: List<Member> = emptyList(),
    val approver: Member? = null,
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
    /** The discount sheet open for the order or one of its lines, if any. */
    val discount: DiscountSheet? = null,
    /** The price sheet open for a line, if any. */
    val price: PriceSheet? = null,
    /** The notes sheet open for a line, if any. */
    val notes: NotesSheet? = null,
    /** This register's drawer as last read; null with none open. */
    val drawer: Drawer? = null,
    /** The last drawer closed, its report, until another opens. */
    val closedDrawer: Drawer? = null,
    /** An amount being typed on the drawer screen. */
    val cash: CashEntry? = null,
    /** The day's report, as last read. */
    val report: DayReport? = null,
    /** Whether the register asks to open the drawer: once someone signs in and none is open. */
    val askDrawer: Boolean = false,
    /** The return being made, on the return screen (ADR-0032). */
    val returning: ReturnSheet? = null,
    /** What the last return refunded, to hand back. */
    val refund: Refund? = null,
    /** The paid orders a receipt's code matched, when more than one did, to choose from. */
    val matches: List<Ticket>? = null,
    /** The change handed back for the last order paid, until the next is begun. */
    val change: String? = null,
    val orders: List<Ticket> = emptyList(),
    /** Where the printer listens, if it is set. */
    val printer: PrinterAddress? = null,
    /** The print jobs, as last read. */
    val jobs: List<PrintJob> = emptyList(),
    /** How replication stands, as last read, for a register joined to its network. */
    val sync: SyncInfo? = null,
    /** The member whose PIN the register asks for, to sign them in. */
    val pinFor: Member? = null,
    /** The manager's approval the register asks for, for an intent refused without one. */
    val approval: Approval? = null,
    /** The version of the profile the team and the menu were read from, once it is known. */
    val profileVersion: String? = null,
    /** What is sold out now, as last read (ADR-0033). */
    val soldOut: List<SoldOutItem> = emptyList(),
    /** The availability version the menu and [soldOut] were read at. */
    val availabilityVersion: Long? = null,
    /** Whether a tap on the menu 86s its item, or puts it back, instead of ringing it. */
    val marking: Boolean = false,
    /** The 86 sheet open for a button, if any. */
    val mark: MarkSheet? = null,
    /** The kitchen's stations, once read: a register with some sends lines to the kitchen. */
    val stations: List<KitchenStation> = emptyList(),
    /** The line sent to the kitchen whose void's reason is being chosen (ADR-0034). */
    val voiding: TicketLine? = null,
    /** The tickets no screen showed in time, as last read: every device tells its staff. */
    val alerts: List<KitchenAlert> = emptyList(),
    /** The kitchen screen, while the device shows it. */
    val kds: KdsState? = null,
    /** The last refusal or failure, until the next intent. */
    val error: String? = null,
    /** This register's device, once the till has said, to tell its orders from other registers'. */
    val device: String? = null,
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
    private val kitchenSettings: KitchenSettings? = null,
) {
    private val mutable = MutableStateFlow(RegisterState())

    /** What the screens show now. */
    val state: StateFlow<RegisterState> = mutable.asStateFlow()

    private val current get() = mutable.value

    /** Loads the team, for whoever signs in, and where the printer is. */
    fun start() = perform {
        val version = version()
        val team = till.team()
        val printer = settings?.load()
        val device = till.deviceId().ifEmpty { null }
        val stations = till.kitchenStations()
        mutable.update {
            it.copy(loading = false, team = team, printer = printer, profileVersion = version, device = device, stations = stations)
        }
    }

    /** The version of the profile the till rings from, or null if it can't say. */
    private fun version(): String? = try {
        till.profileVersion()
    } catch (cancelled: CancellationException) {
        throw cancelled
    } catch (_: Exception) {
        null
    }

    /** Signs [member] in, asking for their PIN first if they have one, and shows the menu. */
    fun signIn(member: Member) {
        if (member.hasPin) {
            mutable.update { it.copy(pinFor = member, error = null) }
        } else {
            perform { completeSignIn(member, "") }
        }
    }

    /** The PIN typed for the member asked for: signs them in, or says it isn't theirs. */
    fun enterPin(pin: String) {
        val member = current.pinFor ?: return
        perform { completeSignIn(member, pin) }
    }

    /** Back to the team, with no one asked for. */
    fun cancelPin() = mutable.update { it.copy(pinFor = null, error = null) }

    /** Who may be asked to approve the approval asked for: those the kernel says may. */
    fun approvers(): List<Member> = current.approval?.approvers.orEmpty()

    /** The manager asked to approve: the register asks for their PIN. */
    fun chooseApprover(approver: Member) =
        mutable.update { state -> state.copy(approval = state.approval?.copy(approver = approver), error = null) }

    /** The PIN typed for the approver: approves, and tries the refused intent again. */
    fun approve(pin: String) {
        val approver = current.approval?.approver ?: return
        val retry = pendingRetry ?: return
        // Refused again, it is the intent that is tried again, never the approval: the PIN is
        // never kept. Found in review.
        perform(retryAs = retry) {
            till.approve(approver.id, pin)
            mutable.update { it.copy(approval = null) }
            pendingRetry = null
            retry()
        }
    }

    /** No approval: the intent stays refused. */
    fun cancelApproval() {
        pendingRetry = null
        mutable.update { it.copy(approval = null, error = null) }
    }

    /** The intent refused for want of an approval, to try again once one is given. */
    private var pendingRetry: (() -> Unit)? = null

    private fun completeSignIn(member: Member, pin: String) {
        till.signIn(member.id, pin)
        // The version the menu is read at, read before it, so a publication taken after it reads
        // it again.
        val version = version()
        // The availability version too, read before what it covers.
        val availability = availability()
        val pages = till.menu()
        val soldOut = till.soldOut()
        // Once someone signs in, the register asks to open the drawer if none is open (ADR-0031).
        val drawer = till.drawer()
        mutable.update {
            it.copy(
                screen = Screen.REGISTER, member = member, pages = pages, page = 0,
                profileVersion = version, pinFor = null, drawer = drawer, askDrawer = drawer == null,
                soldOut = soldOut, availabilityVersion = availability,
            )
        }
    }

    /** The till's availability version, or null if it can't say. */
    private fun availability(): Long? = try {
        till.availabilityVersion()
    } catch (cancelled: CancellationException) {
        throw cancelled
    } catch (_: Exception) {
        null
    }

    /**
     * Reads what is sold out again, and the menu, the modifier sheet and the 86 sheet with it:
     * after an 86 here, or when another register's arrives (ADR-0033). A choice on the open sheet
     * of a modifier now sold out is dropped.
     */
    private fun readAvailability() {
        if (current.member == null) return
        val availability = availability()
        val pages = till.menu()
        val soldOut = till.soldOut()
        val read = current.sheet?.let { till.item(it.item.variant) }
        val mark = current.mark?.let { marked(it.button.variant, pages) }
        // Applied to the state as it is now, not as it was read: what the cashier chose on the
        // sheet meanwhile stays. Found in review: a refresh put back the sheet it began with.
        mutable.update { state ->
            val open = state.sheet
            val button = open?.let { sheet -> pages.flatMap { it.buttons }.firstOrNull { it.variant == sheet.item.variant } }
            // The item itself sold out, or gone from the menu: its sheet closes, saying so.
            val gone = open != null && (button == null || button.soldOut)
            val sheet = when {
                open == null || gone -> null
                read != null && read.variant == open.item.variant -> open.refreshed(read)
                else -> open
            }
            state.copy(
                pages = pages, page = state.page.coerceIn(0, (pages.size - 1).coerceAtLeast(0)), soldOut = soldOut,
                availabilityVersion = availability,
                sheet = sheet,
                mark = state.mark?.let { now ->
                    if (mark != null && mark.button.variant == now.button.variant) mark.copy(working = now.working) else now
                },
                error = if (gone) "${open?.item?.name} just sold out." else state.error,
            )
        }
    }

    /** The 86 sheet for [variant], as [pages] show it: none if the menu no longer has it. */
    private fun marked(variant: String, pages: List<MenuPage>): MarkSheet? {
        val button = pages.flatMap { it.buttons }.firstOrNull { it.variant == variant } ?: return null
        val modifiers = if (button.choices) {
            till.item(variant).groups.flatMap { it.modifiers }.distinctBy { it.modifier }
        } else {
            emptyList()
        }
        return MarkSheet(button, modifiers)
    }

    /** Taps on the menu 86 their items, or ring them again. */
    fun toggleMarking() = mutable.update { it.copy(marking = !it.marking, mark = null, error = null) }

    /**
     * 86s [item], the sheet's variant or, if [modifier], one of its modifiers, for the rest of the
     * day or [untilPutBack] (ADR-0033); tried again as it was once a manager approves.
     */
    fun eightySix(item: String, modifier: Boolean, untilPutBack: Boolean) =
        changeAvailability { till.eightySix(item, modifier, untilPutBack) }

    /** Puts [item], the sheet's variant or, if [modifier], one of its modifiers, back on sale. */
    fun putBack(item: String, modifier: Boolean) = changeAvailability { till.putBack(item, modifier) }

    /**
     * Makes [change] once, the sheet marked working until it is done, so a second tap changes
     * nothing more; and reads what is sold out again.
     */
    private fun changeAvailability(change: () -> Unit) {
        val sheet = current.mark ?: return
        if (sheet.working) return
        mutable.update { it.copy(mark = sheet.copy(working = true), error = null) }
        perform {
            // Working until the sheet shows what the change did. Found in review: its buttons came
            // back before it did, for a second tap to be refused.
            try {
                change()
                readAvailability()
            } finally {
                mutable.update { state -> state.copy(mark = state.mark?.copy(working = false)) }
            }
        }
    }

    /** Closes the 86 sheet. */
    fun closeMark() = mutable.update { it.copy(mark = null) }

    /** Shows the drawer screen, and reads the drawer. */
    fun showDrawer() = perform {
        val drawer = till.drawer()
        // A report read before is gone: read again, it says what is so now. Found in review.
        mutable.update { it.copy(screen = Screen.DRAWER, drawer = drawer, askDrawer = false, report = null) }
    }

    /** Not now: the register stops asking to open the drawer until the next sign-in. */
    fun dismissDrawer() = mutable.update { it.copy(askDrawer = false) }

    /**
     * Begins typing an amount for [purpose], or choosing a no-sale's reason. The register goes on
     * asking to open the drawer until one opens. Found in review: a float cancelled stopped it.
     */
    fun beginCash(purpose: CashPurpose) = mutable.update {
        it.copy(cash = CashEntry(purpose), error = null)
    }

    /** The reason for the movement being typed. */
    fun cashReason(code: String) = mutable.update { state -> state.copy(cash = state.cash?.copy(reason = code)) }

    /** Types [digit] into the amount, in minor units. */
    fun typeCashDigit(digit: Int) = perform {
        val entry = current.cash ?: return@perform
        val typed = (entry.typed?.minor ?: 0) * 10 + digit.coerceIn(0, 9)
        if (typed > MAX_TYPED) return@perform
        val amount = till.amount(typed)
        mutable.update { state -> state.copy(cash = state.cash?.copy(typed = amount)) }
    }

    /** Takes the last digit typed off the amount. */
    fun eraseCashDigit() = perform {
        val entry = current.cash ?: return@perform
        val typed = (entry.typed?.minor ?: 0) / 10
        val amount = if (typed == 0L) null else till.amount(typed)
        mutable.update { state -> state.copy(cash = state.cash?.copy(typed = amount)) }
    }

    /** Closes the keypad without doing anything. */
    fun cancelCash() = mutable.update { it.copy(cash = null) }

    /**
     * Does what the amount was typed for, nothing typed being zero: opens the drawer with it as
     * the float, moves it, or closes the drawer with it as the count; or opens it with no sale.
     * Read once the digits tapped before it are in: read at the tap, a count could close short.
     * Closed then, as the other sheets are, and tried again as it was once a manager approves.
     * Found in review.
     */
    fun confirmCash() = perform {
        val entry = current.cash ?: return@perform
        val minor = entry.typed?.minor ?: 0
        if (entry.purpose.moves && minor <= 0) {
            // The drawer refuses moving nothing: the keypad stays, to type an amount.
            mutable.update { it.copy(error = "Type an amount.") }
            return@perform
        }
        mutable.update { it.copy(cash = null) }
        perform { cash(entry, minor) }
    }

    private fun cash(entry: CashEntry, minor: Long) {
        val drawer = when (entry.purpose) {
            CashPurpose.FLOAT -> {
                val opened = till.openDrawer(minor)
                mutable.update { it.copy(closedDrawer = null, askDrawer = false) }
                opened
            }
            CashPurpose.PAID_IN -> till.moveCash(CashMovement.PAID_IN, minor, entry.reason)
            CashPurpose.PAID_OUT -> till.moveCash(CashMovement.PAID_OUT, minor, entry.reason)
            CashPurpose.DROP -> till.moveCash(CashMovement.DROP, minor, entry.reason)
            CashPurpose.NO_SALE -> till.noSale(entry.reason)
            CashPurpose.COUNT -> {
                val closed = till.closeDrawer(minor)
                mutable.update { it.copy(drawer = null, closedDrawer = closed, report = null) }
                return
            }
        }
        mutable.update { it.copy(drawer = drawer, report = null) }
    }

    /** Opens the drawer with no sale, for [reason]. */
    fun noSale(reason: String) = perform {
        mutable.update { it.copy(drawer = till.noSale(reason), report = null) }
    }

    /** Reads today's X or Z report. */
    fun showReport(z: Boolean) = perform {
        mutable.update { it.copy(report = till.dayReport(z)) }
    }

    /** Prints today's X or Z report. */
    fun printReport(z: Boolean) = perform {
        till.printReport(z)
        val jobs = till.printJobs()
        mutable.update { it.copy(jobs = jobs) }
    }

    /**
     * Signs out: the next cashier signs in. An order being rung stays open and on the screen, for
     * them to go on with; the sheet and the tender close. Found in review: it left the screen, and
     * the next tap began another order.
     */
    fun signOut() = perform {
        till.signOut()
        mutable.update {
            RegisterState(
                loading = false, team = it.team, printer = it.printer, ticket = it.ticket, sync = it.sync,
                profileVersion = it.profileVersion, device = it.device, stations = it.stations,
            )
        }
    }

    /** Shows the menu's page [index]. */
    fun showPage(index: Int) = mutable.update { state ->
        state.copy(page = index.coerceIn(0, (state.pages.size - 1).coerceAtLeast(0)))
    }

    /**
     * Rings [button]'s item, or opens its modifier sheet if it has modifiers to choose; while
     * marking, opens its 86 sheet instead. An item sold out isn't rung (ADR-0033).
     */
    fun tap(button: MenuButton) {
        // Marking or not as at the tap, not when the worker gets to it. Found in review: a tap
        // made marking, then Done, rang the item.
        val marking = current.marking
        perform { tapped(button, marking) }
    }

    private fun tapped(button: MenuButton, marking: Boolean) {
        if (marking) {
            val sheet = marked(button.variant, current.pages) ?: return
            mutable.update { it.copy(mark = sheet) }
            return
        }
        if (button.soldOut) throw TillException("${button.name} is sold out ${backWords(button.backAt)}.")
        if (button.choices) {
            val item = till.item(button.variant)
            mutable.update { it.copy(sheet = ModifierSheet(item)) }
        } else {
            ring(button.variant, emptyList(), 1)
        }
    }

    /** Taps [modifier] in group [group], its place in the item's groups, on the open sheet. */
    fun choose(group: Int, modifier: String) = mutable.update { state ->
        state.copy(sheet = state.sheet?.tap(group, modifier), error = null)
    }

    /** Sets how many of the sheet's item to ring. */
    fun sheetQuantity(quantity: Int) = mutable.update { state ->
        state.copy(sheet = state.sheet?.withQuantity(quantity.coerceAtMost(MAX_QUANTITY)))
    }

    /** Closes the sheet without ringing anything. */
    fun closeSheet() = mutable.update { it.copy(sheet = null) }

    /** Rings the sheet's item with what was chosen, and closes it. */
    fun addFromSheet() = perform {
        val sheet = current.sheet ?: return@perform
        ring(sheet.item.variant, sheet.choices(), sheet.quantity)
        mutable.update { it.copy(sheet = null) }
    }

    /** One more of [line]; a line sent to the kitchen isn't changed: another is rung. */
    fun more(line: TicketLine) = perform {
        val (order, now) = current(line) ?: return@perform
        if (now.sent) throw TillException("${now.name} was sent to the kitchen: ring another.")
        val count = now.count ?: return@perform
        show(till.changeQuantity(order, now.line, count + 1))
    }

    /** One fewer of [line]: the last one takes the line off; one sent is voided, for a reason. */
    fun fewer(line: TicketLine) = perform {
        val (order, now) = current(line) ?: return@perform
        if (now.sent) return@perform beginVoid(now)
        val count = now.count ?: return@perform
        show(if (count > 1) till.changeQuantity(order, now.line, count - 1) else till.removeLine(order, now.line))
    }

    /** Takes [line] off the order; one sent to the kitchen is voided, for a reason (ADR-0034). */
    fun remove(line: TicketLine) = perform {
        val (order, now) = current(line) ?: return@perform
        if (now.sent) return@perform beginVoid(now)
        show(till.removeLine(order, now.line))
    }

    private fun beginVoid(line: TicketLine) = mutable.update { it.copy(voiding = line, error = null) }

    /**
     * Voids the line asked about, for [reason]: closed at the tap, so a second tap voids nothing
     * more; tried again as it was once a manager approves.
     */
    fun voidLine(reason: String) {
        val line = current.voiding ?: return
        val order = current.ticket?.order ?: return
        mutable.update { it.copy(voiding = null) }
        perform { show(till.voidLine(order, line.line, reason)) }
    }

    /** No void: the line stays. */
    fun closeVoid() = mutable.update { it.copy(voiding = null) }

    /** Sends the order's pending lines to the kitchen (ADR-0034). */
    fun send() = perform {
        val ticket = current.ticket ?: return@perform
        show(till.send(ticket.order))
    }

    /**
     * Shows the kitchen screen (ADR-0034): the stations kept on the device, or every one, and
     * their tickets. No one signs in; the register's own sign-in waits.
     */
    fun openKitchen() = perform {
        val stations = till.kitchenStations()
        if (stations.isEmpty()) throw TillException("This location has no kitchen.")
        val known = stations.map { it.station }.toSet()
        val kept = kitchenSettings?.load()?.intersect(known)?.takeIf { it.isNotEmpty() } ?: known
        mutable.update { it.copy(screen = Screen.KITCHEN, stations = stations, kds = KdsState(kept)) }
        readKitchen()
    }

    /** Reads the kitchen screen again; the shell calls it every second, as tickets age. */
    fun refreshKitchen() = scope.launch {
        if (current.kds == null) return@launch
        try {
            withContext(worker) { readKitchen() }
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (_: Exception) {
            // Read again at the next second.
        }
    }

    /** Reads what the screen's stations show, keeping the selection on the ticket it was on. */
    private fun readKitchen() {
        val kds = current.kds ?: return
        val view = till.kitchenScreen(kds.stations.toList())
        mutable.update { state ->
            val now = state.kds ?: return@update state
            val was = now.selectedTicket?.ticket
            val at = view.tickets.indexOfFirst { it.ticket == was }.takeIf { it >= 0 }
                ?: now.selected.coerceIn(0, (view.tickets.size - 1).coerceAtLeast(0))
            state.copy(kds = now.copy(view = view, selected = at, bumping = now.bumping.filter { id -> view.tickets.any { it.ticket == id } }.toSet()))
        }
    }

    /** Bumps [ticket] once, however many times it is tapped or its key pressed. */
    fun bump(ticket: KdsTicket) {
        val kds = current.kds ?: return
        if (ticket.ticket in kds.bumping) return
        mutable.update { state -> state.copy(kds = state.kds?.copy(bumping = state.kds.bumping + ticket.ticket), error = null) }
        perform {
            try {
                till.bump(ticket.ticket)
            } finally {
                mutable.update { state -> state.copy(kds = state.kds?.copy(bumping = state.kds.bumping - ticket.ticket)) }
            }
            readKitchen()
        }
    }

    /** Bumps the ticket selected: a bump bar's bump key. */
    fun bumpSelected() {
        val ticket = current.kds?.selectedTicket ?: return
        bump(ticket)
    }

    /** Moves the selection [by] tickets: a bump bar's arrow keys. */
    fun select(by: Int) = mutable.update { state ->
        val kds = state.kds ?: return@update state
        val last = (kds.view.tickets.size - 1).coerceAtLeast(0)
        state.copy(kds = kds.copy(selected = (kds.selected + by).coerceIn(0, last)))
    }

    /** Selects [ticket]: a tap. */
    fun select(ticket: KdsTicket) = mutable.update { state ->
        val kds = state.kds ?: return@update state
        val at = kds.view.tickets.indexOfFirst { it.ticket == ticket.ticket }
        if (at < 0) state else state.copy(kds = kds.copy(selected = at))
    }

    /** Brings back the ticket bumped last: a bump bar's recall key. */
    fun recallLast() = perform {
        val last = current.kds?.view?.lastBumped ?: throw TillException("Nothing was bumped today.")
        till.recallTicket(last)
        readKitchen()
    }

    /** Opens or closes the choice of stations. */
    fun chooseStations() = mutable.update { state -> state.copy(kds = state.kds?.copy(choosing = !state.kds.choosing)) }

    /** Shows [station] on the screen, or stops: at least one is always shown. */
    fun toggleStation(station: String) = perform {
        val kds = current.kds ?: return@perform
        val chosen = if (station in kds.stations) kds.stations - station else kds.stations + station
        if (chosen.isEmpty()) return@perform
        kitchenSettings?.save(chosen)
        mutable.update { state -> state.copy(kds = state.kds?.copy(stations = chosen, selected = 0)) }
        readKitchen()
    }

    /** Leaves the kitchen screen, for signing in. */
    fun closeKitchen() = mutable.update { it.copy(screen = Screen.SIGN_IN, kds = null, error = null) }

    /**
     * [line] as the order being rung has it now, on the worker, after every intent queued before:
     * a second tap counts from the first's answer, not from the screen it was made on. Found in
     * review: two quick taps of + made the line 2, not 3. None if it is gone.
     */
    private fun current(line: TicketLine): Pair<String, TicketLine>? {
        val ticket = current.ticket ?: return null
        return ticket.lines.firstOrNull { it.line == line.line }?.let { ticket.order to it }
    }

    /** Opens the discount sheet for [line], or for the whole order when it is null. */
    fun openDiscount(line: TicketLine? = null) {
        if (current.ticket == null) return
        mutable.update { it.copy(discount = DiscountSheet(line), error = null) }
    }

    /** The reason for the discount on the sheet. */
    fun discountReason(code: String) =
        mutable.update { state -> state.copy(discount = state.discount?.copy(reason = code)) }

    /** Types [digit] into the sheet's amount, in minor units. */
    fun typeDiscountDigit(digit: Int) = perform {
        val sheet = current.discount ?: return@perform
        val typed = (sheet.typed?.minor ?: 0) * 10 + digit.coerceIn(0, 9)
        if (typed > MAX_TYPED) return@perform
        val amount = till.amount(typed)
        mutable.update { state -> state.copy(discount = state.discount?.copy(typed = amount)) }
    }

    /** Takes the last digit typed off the sheet's amount. */
    fun eraseDiscountDigit() = perform {
        val sheet = current.discount ?: return@perform
        val typed = (sheet.typed?.minor ?: 0) / 10
        val amount = if (typed == 0L) null else till.amount(typed)
        mutable.update { state -> state.copy(discount = state.discount?.copy(typed = amount)) }
    }

    /** Takes [basisPoints] off the sheet's line or order, and closes the sheet. */
    fun discountPercent(basisPoints: Int) = discount(DiscountAsked.Percent(basisPoints))

    /** Takes the amount typed off the sheet's line, and closes the sheet. A whole order takes a percentage. */
    fun discountAmount() {
        val sheet = current.discount ?: return
        val typed = sheet.typed ?: return
        if (sheet.line == null) {
            mutable.update { it.copy(error = "A whole order takes a percentage off.") }
            return
        }
        discount(DiscountAsked.Amount(typed.minor))
    }

    /**
     * Asks for [asked] as the sheet says, the sheet's line and reason taken now: tried again as
     * they were once a manager approves.
     */
    private fun discount(asked: DiscountAsked) {
        val sheet = current.discount ?: return
        val order = current.ticket?.order ?: return
        // Closed at the tap, so a second tap before the kernel answers finds nothing to apply.
        // Found in review: a double tap of 10% put on two discounts.
        mutable.update { it.copy(discount = null) }
        perform { show(till.applyDiscount(order, sheet.line?.line, asked, sheet.reason)) }
    }

    /** Closes the discount sheet. */
    fun closeDiscount() = mutable.update { it.copy(discount = null) }

    /** Takes [discount] off the order being rung. */
    fun removeDiscount(discount: TicketDiscount) = perform {
        val ticket = current.ticket ?: return@perform
        show(till.removeDiscount(ticket.order, discount.discount))
    }

    /** Opens the price sheet for [line]. */
    fun openPrice(line: TicketLine) {
        if (current.ticket == null) return
        mutable.update { it.copy(price = PriceSheet(line), error = null) }
    }

    /** The reason for the price on the sheet. */
    fun priceReason(code: String) = mutable.update { state -> state.copy(price = state.price?.copy(reason = code)) }

    /** Types [digit] into the sheet's price, in minor units. */
    fun typePriceDigit(digit: Int) = perform {
        val sheet = current.price ?: return@perform
        val typed = (sheet.typed?.minor ?: 0) * 10 + digit.coerceIn(0, 9)
        if (typed > MAX_TYPED) return@perform
        val amount = till.amount(typed)
        mutable.update { state -> state.copy(price = state.price?.copy(typed = amount)) }
    }

    /** Takes the last digit typed off the sheet's price. */
    fun erasePriceDigit() = perform {
        val sheet = current.price ?: return@perform
        val typed = (sheet.typed?.minor ?: 0) / 10
        val amount = if (typed == 0L) null else till.amount(typed)
        mutable.update { state -> state.copy(price = state.price?.copy(typed = amount)) }
    }

    /**
     * Prices the sheet's line at what was typed, nothing typed being free, and closes the sheet:
     * tried again as it was once a manager approves.
     */
    fun setPrice() = perform {
        // Read once the digits tapped before it are in, as the drawer's keypad is. Found in review.
        val sheet = current.price ?: return@perform
        val order = current.ticket?.order ?: return@perform
        val minor = sheet.typed?.minor ?: 0
        mutable.update { it.copy(price = null) }
        perform { show(till.repriceLine(order, sheet.line.line, minor, sheet.reason)) }
    }

    /** Closes the price sheet. */
    fun closePrice() = mutable.update { it.copy(price = null) }

    /** Opens the notes sheet for [line]. */
    fun openNotes(line: TicketLine) {
        if (current.ticket == null) return
        mutable.update { it.copy(notes = NotesSheet(line), error = null) }
    }

    /** Keeps [text] as the sheet's line's notes, blank clearing them, and closes the sheet. */
    fun saveNotes(text: String) {
        val sheet = current.notes ?: return
        val order = current.ticket?.order ?: return
        val notes = text.trim().ifEmpty { null }
        mutable.update { it.copy(notes = null) }
        if (notes == sheet.line.notes) return
        perform { show(till.setNotes(order, sheet.line.line, notes)) }
    }

    /** Closes the notes sheet. */
    fun closeNotes() = mutable.update { it.copy(notes = null) }

    /** Drops the order being rung. */
    fun abandon() = perform {
        val ticket = current.ticket ?: return@perform
        till.abandon(ticket.order)
        mutable.update { it.copy(ticket = null, tender = null, discount = null, price = null, notes = null) }
    }

    /** Opens the cash tender for the order being rung, or for its [check] when it is split. */
    fun openTender(check: TicketCheck? = null) = perform {
        val ticket = current.ticket ?: return@perform
        val tender = if (check == null) {
            val offers = till.cashOffers(ticket.order)
            val due = offers.firstOrNull() ?: till.amount(ticket.cashDueMinor)
            Tender(ticket.order, due, offers)
        } else {
            // The check as the ticket has it now, after every intent queued before: not as it was
            // drawn. Found in review.
            val now = ticket.checks.firstOrNull { it.check == check.check } ?: return@perform
            if (!now.open) return@perform
            val due = till.amount(now.cashDueMinor)
            Tender(ticket.order, due, roundUp(now.cashDueMinor).map(till::amount), check = now)
        }
        mutable.update { it.copy(tender = tender) }
    }

    /** What is due, then the next whole 5, 10 and 20 above it, as a cash drawer takes them. */
    private fun roundUp(due: Long): List<Long> =
        (listOf(due) + listOf(500L, 1_000L, 2_000L).map { (due / it + 1) * it }).distinct()

    /** Splits the order being rung evenly among [ways] checks. */
    fun split(ways: Int) = perform {
        val ticket = current.ticket ?: return@perform
        show(till.splitEvenly(ticket.order, ways))
    }

    /** Moves [line] to the check [to], or to a new one when it is null. */
    fun moveLine(line: TicketLine, to: String?) = perform {
        val (order, now) = current(line) ?: return@perform
        show(till.moveLine(order, now.line, to))
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

    /** Pays the order, or the tender's check, with [tendered], and shows the change. */
    fun pay(tendered: Amount) = perform {
        val tender = current.tender ?: return@perform
        val check = tender.check
        val paid = if (check == null) {
            till.payCash(tender.order, tendered.minor)
        } else {
            till.payCheckCash(tender.order, check.check, null, tendered.minor)
        }
        paid(paid)
    }

    /** Pays what was typed toward the tender's check, as a part of it, in exact cash. */
    fun payPart() = perform {
        val tender = current.tender ?: return@perform
        val check = tender.check ?: return@perform
        val typed = tender.typed ?: return@perform
        paid(till.payCheckCash(tender.order, check.check, typed.minor, typed.minor))
    }

    /** Shows what paying did: an order still open stays on the screen, with what it owes now. */
    private fun paid(paid: Paid) {
        val open = paid.ticket.state == OrderState.OPEN
        mutable.update {
            it.copy(
                ticket = if (open) paid.ticket else null, tender = null, discount = null, price = null,
                notes = null, change = paid.change,
            )
        }
    }

    /** Shows today's orders. */
    fun showOrders() = perform {
        val orders = listed()
        mutable.update { it.copy(screen = Screen.ORDERS, orders = orders) }
    }

    /**
     * Today's orders, and every order still open from an earlier day, as one parked on another
     * register before the day turned. Found in review: those couldn't be recalled.
     */
    private fun listed(): List<Ticket> {
        val today = till.ordersToday()
        val earlier = till.openOrders().filter { open -> today.none { it.order == open.order } }
        return earlier + today
    }

    /**
     * Takes up [order] again, if it is open, on the register. One parked on another device is
     * recalled: the register asks the hub for it, and takes it up once the hub grants it
     * (ADR-0030); until then it stays in the list, waiting.
     */
    fun resume(order: Ticket) = perform {
        val ticket = till.ticket(order.order)
        if (ticket.state != OrderState.OPEN) throw TillException("That order isn't open.")
        if (elsewhere(ticket)) {
            till.recall(ticket.order)
            val orders = listed()
            mutable.update { it.copy(orders = orders, error = "Asked the hub for that order: open it again once it's here.") }
            return@perform
        }
        mutable.update { it.copy(screen = Screen.REGISTER, ticket = ticket, change = null) }
    }

    /**
     * Whether [ticket] is owned by another device than this register: read from the state, not the
     * till, so the screen asks the kernel nothing. Found in review.
     */
    fun elsewhere(ticket: Ticket): Boolean {
        val owner = ticket.owner ?: return false
        val me = current.device ?: return false
        return owner != me
    }

    /** Whether this register is among those that asked the hub for [ticket]. */
    fun askedHere(ticket: Ticket): Boolean = current.device?.let { it in ticket.asking } ?: false

    /** Takes [order] over from its device on a manager's word, then takes it up. */
    fun takeOver(order: Ticket) = perform {
        val ticket = till.takeOver(order.order)
        mutable.update { it.copy(screen = Screen.REGISTER, ticket = ticket, change = null) }
    }

    /** Prints the closed [order]'s receipt again. */
    fun reprint(order: Ticket) = perform {
        if (order.state != OrderState.CLOSED) throw TillException("Only a paid order has a receipt.")
        till.reprint(order.order)
        val jobs = till.printJobs()
        mutable.update { it.copy(jobs = jobs) }
    }

    /**
     * Finds the paid order whose receipt code is [code], six characters, and begins its return;
     * with more than one, lists them for the cashier to choose (ADR-0032). Found in review: a
     * shorter code opened the latest order it ended, which might be any.
     */
    fun findReturn(code: String) = perform {
        if (code.trim().length != RECEIPT_CODE) throw TillException("A receipt's code is six characters.")
        val found = till.findOrders(code)
        when (found.size) {
            0 -> throw TillException("No paid order has that code.")
            1 -> openReturn(till.returnable(found.single()))
            else -> {
                val matches = found.map(till::ticket)
                mutable.update { it.copy(matches = matches) }
            }
        }
    }

    /** Begins a return of paid order [order]. */
    fun beginReturn(order: Ticket) = perform {
        if (order.state != OrderState.CLOSED) throw TillException("Only a paid order is returned.")
        openReturn(till.returnable(order.order))
    }

    private fun openReturn(returnable: Returnable) {
        mutable.update {
            it.copy(screen = Screen.RETURN, returning = ReturnSheet(returnable), refund = null, matches = null)
        }
    }

    /** Picks one more of [line] to return, up to what is left; a line sold by measure, whole. */
    fun pickMore(line: ReturnableLine) = mutable.update { state ->
        val sheet = state.returning ?: return@update state
        if (!line.anyLeft || sheet.refunding || !sheet.returnable.cashOnly) return@update state
        val left = line.leftCount
        val picks = if (left == null) {
            sheet.picks + (line.line to null)
        } else {
            val now = sheet.picks[line.line] ?: 0
            sheet.picks + (line.line to minOf(now + 1, left))
        }
        state.copy(returning = sheet.copy(picks = picks), error = null)
    }

    /** Picks one fewer of [line]: none, for a line sold by measure. */
    fun pickFewer(line: ReturnableLine) = mutable.update { state ->
        val sheet = state.returning ?: return@update state
        if (sheet.refunding) return@update state
        val now = sheet.picks[line.line]
        val picks = if (now == null || now <= 1) sheet.picks - line.line else sheet.picks + (line.line to now - 1)
        state.copy(returning = sheet.copy(picks = picks))
    }

    /** Why the lines are returned. */
    fun returnReason(code: String) = mutable.update { state ->
        state.copy(returning = state.returning?.copy(reason = code), error = null)
    }

    /**
     * Returns what is picked, for the reason chosen, and shows what to hand back; tried again as
     * it was once a manager approves. Taken at the tap, and nothing more is taken until it is
     * done: found in review, a second tap refunded again.
     */
    fun confirmReturn() {
        val sheet = current.returning ?: return
        if (sheet.refunding) return
        val reason = sheet.reason
        val refusal = when {
            sheet.picks.isEmpty() -> "Choose what is returned."
            reason == null -> "Choose why it is returned."
            else -> null
        }
        if (refusal != null || reason == null) {
            mutable.update { it.copy(error = refusal) }
            return
        }
        val picks = sheet.picks.map { (line, count) -> ReturnPick(line, count) }
        val order = sheet.returnable.order
        mutable.update { it.copy(returning = sheet.copy(refunding = true), error = null) }
        perform {
            val refund = try {
                till.returnLines(order, picks, reason)
            } catch (refused: TillException) {
                // What is left may have changed, as another register returned some of it: read it
                // again, and keep only what can still be picked. Found in review.
                val returnable = if (refused.approval == null) till.returnable(order) else null
                mutable.update { state ->
                    val now = state.returning ?: return@update state
                    val read = returnable ?: now.returnable
                    state.copy(returning = now.copy(returnable = read, picks = clamp(now.picks, read), refunding = false))
                }
                throw refused
            }
            // The refund is shown, and its picks gone, before anything else can fail: found in
            // review, a failed refresh left them for a second tap.
            mutable.update { state ->
                state.copy(refund = refund, returning = state.returning?.copy(picks = emptyMap(), refunding = false))
            }
            val returnable = till.returnable(order)
            val jobs = till.printJobs()
            mutable.update { state -> state.copy(returning = state.returning?.copy(returnable = returnable), jobs = jobs) }
        }
    }

    /** [picks], each no more than [returnable] has left. */
    private fun clamp(picks: Map<String, Long?>, returnable: Returnable): Map<String, Long?> =
        picks.mapNotNull { (line, count) ->
            val known = returnable.lines.find { it.line == line }?.takeIf { it.anyLeft } ?: return@mapNotNull null
            val left = known.leftCount
            line to if (left == null || count == null) count else minOf(count, left)
        }.toMap()

    /** Leaves the return: back to today's orders, at once, which are then read again. */
    fun closeReturn() {
        mutable.update { it.copy(screen = Screen.ORDERS, returning = null, refund = null, matches = null, error = null) }
        perform {
            val orders = listed()
            mutable.update { it.copy(orders = orders) }
        }
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
        val sync = try {
            withContext(worker) { till.syncState() }
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (_: Exception) {
            null
        }
        mutable.update { it.copy(sync = sync) }
        // The till's own version, joined or not: the team and the menu are read again whenever it
        // isn't the one they were read at, an unknown one included. Found in review: a register
        // that took a publication as it joined, after its cashier signed in, kept the old menu.
        val version = withContext(worker) { version() }
        if (version != null && version != current.profileVersion) catalogChanged(version)
        // The tickets no screen showed in time, which every device tells its staff (ADR-0034).
        if (current.stations.isNotEmpty()) {
            val alerts = try {
                withContext(worker) { till.kitchenAlerts() }
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (_: Exception) {
                current.alerts
            }
            mutable.update { it.copy(alerts = alerts) }
        }
        // What is sold out, read again whenever it moves: an 86 on another register, or one whose
        // time has passed, which moves no version (ADR-0033).
        if (current.member == null) return@launch
        val moved = try {
            withContext(worker) {
                till.availabilityVersion() != current.availabilityVersion || till.soldOut() != current.soldOut
            }
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (_: Exception) {
            false
        }
        // Read on the worker, as intents are, but as a poll: an error the cashier is reading
        // stays, and a read that fails says nothing. Found in review: each 86 in the store
        // cleared it, and a failed read showed every 2 s.
        if (!moved) return@launch
        try {
            withContext(worker) { readAvailability() }
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (_: Exception) {
            // Read again at the next poll.
        }
    }

    /**
     * The cloud published a profile, which the kernel took between two intents: the team and the
     * menu are read again. A cashier the new team doesn't have was signed out by the kernel, and
     * the register goes back to signing in.
     */
    private fun catalogChanged(version: String) = perform {
        val team = till.team()
        val member = current.member
        if (member != null && team.none { it.id == member.id }) {
            mutable.update {
                RegisterState(
                    loading = false, team = team, printer = it.printer, ticket = it.ticket,
                    sync = it.sync, profileVersion = version, device = it.device,
                )
            }
            return@perform
        }
        val pages = if (member != null) till.menu() else current.pages
        mutable.update {
            it.copy(
                team = team, pages = pages, page = it.page.coerceIn(0, (pages.size - 1).coerceAtLeast(0)),
                profileVersion = version,
            )
        }
        // The new catalog's items, on the open sheets too. Found in review.
        readAvailability()
    }

    /** Back to the register from today's orders or the printer. */
    fun showRegister() = mutable.update { it.copy(screen = Screen.REGISTER, error = null, matches = null) }

    /** Rings [variant], on a new order if none is open. */
    private fun ring(variant: String, choices: List<Chosen>, quantity: Int) {
        // An order begun is shown at once, so a line it refuses doesn't leave it behind.
        val order = current.ticket?.order ?: till.startOrder().also(::show).order
        show(till.addItem(order, variant, choices, quantity))
    }

    private fun show(ticket: Ticket) = mutable.update { it.copy(ticket = ticket, change = null) }

    /**
     * Runs [intent] on the worker, clearing the last refusal first and keeping a new one. One that
     * needs an approval is kept, as [retryAs] if given, to try again once one is given.
     */
    private fun perform(retryAs: (() -> Unit)? = null, intent: () -> Unit) {
        mutable.update { it.copy(error = null) }
        scope.launch {
            try {
                withContext(worker) { intent() }
            } catch (refused: TillException) {
                val action = refused.approval
                if (action != null) {
                    // Asked again once a manager approves.
                    pendingRetry = retryAs ?: intent
                    val ids = withContext(worker) { till.approvers() }
                    val approvers = current.team.filter { it.id in ids }
                    mutable.update {
                        it.copy(loading = false, approval = Approval(action, refused.message.orEmpty(), approvers))
                    }
                } else {
                    mutable.update { it.copy(loading = false, error = refused.message) }
                }
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

        /** The most of an item the sheet rings at once. */
        const val MAX_QUANTITY = 999
    }
}
