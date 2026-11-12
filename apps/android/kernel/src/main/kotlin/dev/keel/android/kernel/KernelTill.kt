package dev.keel.android.kernel

import dev.keel.kernel.Choice
import dev.keel.kernel.KeelException
import dev.keel.kernel.Mode
import dev.keel.kernel.Prefix
import dev.keel.kernel.Register
import dev.keel.kernel.RegisterConfig
import dev.keel.kernel.Resolution
import dev.keel.kernel.JobKind as KernelJobKind
import dev.keel.kernel.JobState as KernelJobState
import dev.keel.kernel.TicketState
import dev.keel.register.Amount
import dev.keel.register.Chosen
import dev.keel.register.Group
import dev.keel.register.Item
import dev.keel.register.JobKind
import dev.keel.register.JobState
import dev.keel.register.PrintJob
import dev.keel.register.PrinterAddress
import dev.keel.kernel.SyncKit
import dev.keel.register.SyncInfo
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import kotlin.time.Duration
import kotlin.time.Duration.Companion.milliseconds
import dev.keel.register.Member
import dev.keel.register.MenuButton
import dev.keel.register.MenuPage
import dev.keel.register.Modifier
import dev.keel.register.OrderState
import dev.keel.register.Paid
import dev.keel.register.Ticket
import dev.keel.register.TicketLine
import dev.keel.register.Till
import dev.keel.register.TillException
import dev.keel.kernel.Amount as KernelAmount
import dev.keel.kernel.Ticket as KernelTicket
import dev.keel.kernel.TicketModifier as KernelModifier
import dev.keel.kernel.TicketDiscount as KernelDiscount
import dev.keel.kernel.TicketLine as KernelLine
import dev.keel.kernel.TicketCheck as KernelCheck
import dev.keel.register.TicketCheck
import dev.keel.register.DiscountAsked
import dev.keel.register.CashMovement
import dev.keel.register.DayReport
import dev.keel.register.Drawer
import dev.keel.kernel.CashMovement as KernelMovement
import dev.keel.kernel.DrawerView as KernelDrawer
import dev.keel.kernel.ReportKind as KernelReportKind
import dev.keel.register.TicketDiscount
import dev.keel.register.percentWords
import dev.keel.register.reasonWords

/** The register's [Till], answered by the kernel's [register], which it closes with itself. */
class KernelTill(private val register: Register) : Till, AutoCloseable {
    companion object {
        /** The till over the register [config] opens, its refusals in the cashier's words. */
        fun open(config: RegisterConfig): KernelTill = call { KernelTill(Register.open(config)) }
    }

    override fun team(): List<Member> = call { register.team().map { Member(it.id, it.name, it.role, it.hasPin) } }

    override fun signIn(member: String, pin: String) = call { register.signIn(member, pin) }

    override fun approve(approver: String, pin: String) = call { register.approve(approver, pin) }

    override fun approvers(): List<String> = register.approvers()

    override fun signOut() = call { register.signOut() }

    override fun menu(): List<MenuPage> = call {
        register.menu().pages.map { page ->
            MenuPage(
                page.name,
                page.buttons.map { MenuButton(it.variant, it.name, it.price.text, it.choices, it.required) },
            )
        }
    }

    override fun item(variant: String): Item = call {
        val item = register.item(variant)
        Item(
            item.variant,
            item.name,
            item.price.text,
            item.groups.map { group ->
                Group(
                    group.group,
                    group.under,
                    group.name,
                    group.min.toInt(),
                    group.max.toInt(),
                    group.free.toInt(),
                    group.repeat,
                    group.modifiers.map { Modifier(it.modifier, it.name, it.price.text) },
                    group.depth.toInt(),
                )
            },
        )
    }

    override fun startOrder(): Ticket = call { register.startOrder(Mode.TAKEOUT).toTicket() }

    override fun addItem(order: String, variant: String, choices: List<Chosen>, quantity: Int): Ticket = call {
        val flattened = choices.map { Choice(it.modifier, it.quantity.toUByte(), it.parent?.toUInt()) }
        register.addItem(order, variant, flattened, quantity.toUInt()).toTicket()
    }

    override fun changeQuantity(order: String, line: String, quantity: Int): Ticket =
        call { register.changeQuantity(order, line, quantity.toUInt()).toTicket() }

    override fun removeLine(order: String, line: String): Ticket = call { register.removeLine(order, line).toTicket() }

    override fun applyDiscount(order: String, line: String?, discount: DiscountAsked, reason: String): Ticket = call {
        when (discount) {
            is DiscountAsked.Percent -> register.applyPercentDiscount(order, line, discount.basisPoints.toUInt(), reason, null)
            is DiscountAsked.Amount -> {
                val on = line ?: throw TillException("A whole order takes a percentage off.")
                register.applyAmountDiscount(order, on, discount.minor, reason, null)
            }
        }.toTicket()
    }

    override fun removeDiscount(order: String, discount: String): Ticket =
        call { register.removeDiscount(order, discount).toTicket() }

    override fun repriceLine(order: String, line: String, unitPriceMinor: Long, reason: String): Ticket =
        call { register.repriceLine(order, line, unitPriceMinor, reason, null).toTicket() }

    override fun setNotes(order: String, line: String, notes: String?): Ticket =
        call { register.setLineNotes(order, line, notes).toTicket() }

    override fun abandon(order: String): Ticket = call { register.abandon(order).toTicket() }

    override fun cashOffers(order: String): List<Amount> = call { register.cashOffers(order).map { it.toAmount() } }

    override fun amount(minor: Long): Amount = call { register.amount(minor).toAmount() }

    override fun payCash(order: String, tenderedMinor: Long): Paid = call {
        val paid = register.payCash(order, tenderedMinor)
        Paid(paid.change.text, paid.ticket.toTicket())
    }

    override fun payCheckCash(order: String, check: String, partMinor: Long?, tenderedMinor: Long): Paid = call {
        val paid = register.payCheckCash(order, check, partMinor, tenderedMinor)
        Paid(paid.change.text, paid.ticket.toTicket())
    }

    override fun splitEvenly(order: String, ways: Int): Ticket =
        call { register.splitEvenly(order, ways.coerceIn(0, 255).toUByte()).toTicket() }

    override fun moveLine(order: String, line: String, to: String?): Ticket =
        call { register.moveLine(order, line, to).toTicket() }

    override fun ticket(order: String): Ticket = call { register.ticket(order).toTicket() }

    override fun drawer(): Drawer? = call { register.drawer()?.toDrawer() }

    override fun openDrawer(floatMinor: Long): Drawer = call { register.openDrawer(floatMinor).toDrawer() }

    override fun moveCash(movement: CashMovement, amountMinor: Long, reason: String): Drawer = call {
        val kind = when (movement) {
            CashMovement.PAID_IN -> KernelMovement.PAID_IN
            CashMovement.PAID_OUT -> KernelMovement.PAID_OUT
            CashMovement.DROP -> KernelMovement.DROP
        }
        register.moveCash(kind, amountMinor, reason, null).toDrawer()
    }

    override fun noSale(reason: String): Drawer = call { register.noSale(reason, null).toDrawer() }

    override fun closeDrawer(countedMinor: Long): Drawer = call { register.closeDrawer(countedMinor).toDrawer() }

    override fun dayReport(z: Boolean): DayReport = call {
        val report = register.dayReport(if (z) KernelReportKind.Z else KernelReportKind.X, null)
        DayReport(
            z = z,
            date = report.date,
            orders = report.orders.toLong(),
            gross = report.gross.text,
            discounts = report.discounts.text,
            net = report.net.text,
            tax = report.tax.text,
            total = report.total.text,
            tenders = report.tenders.map { tender ->
                val name = if (tender.tender == "cash") "Cash" else "Card"
                "$name (${tender.payments})" to tender.amount.text
            },
            voided = report.voided.toLong(),
            drawers = report.drawers.map { it.toDrawer() },
            withoutDrawer = report.cashWithoutDrawer.takeIf { report.paymentsWithoutDrawer > 0uL }
                ?.let { "Cash with no drawer (${report.paymentsWithoutDrawer})" to it.text },
        )
    }

    override fun printReport(z: Boolean) =
        call { register.printReport(if (z) KernelReportKind.Z else KernelReportKind.X, null) }.let { }

    override fun recall(order: String): Ticket = call { register.recall(order).toTicket() }

    /**
     * The runtime gates a take-over as `order.void` (ADR-0030), so its refusals name that action:
     * told here as taking an order over. Found in review: they read as dropping an order.
     */
    override fun takeOver(order: String): Ticket = try {
        call { register.takeOver(order, "hub_unreachable", null).toTicket() }
    } catch (refused: TillException) {
        val words = when (refused.cause) {
            is KeelException.ApprovalRequired -> "Taking an order over needs a manager's approval."
            is KeelException.NotApprover -> "That manager can't approve taking an order over."
            else -> throw refused
        }
        throw TillException(words, refused.cause, refused.approval)
    }

    override fun deviceId(): String = register.deviceId()

    override fun ordersToday(): List<Ticket> = call { register.ordersToday().map { it.toTicket() } }

    override fun openOrders(): List<Ticket> = call { register.openOrders().map { it.toTicket() } }

    override fun printJobs(): List<PrintJob> = call {
        register.printJobs().map { job ->
            PrintJob(
                job.job,
                when (job.kind) {
                    KernelJobKind.RECEIPT -> JobKind.RECEIPT
                    KernelJobKind.DRAWER -> JobKind.DRAWER
                },
                when (job.state) {
                    KernelJobState.QUEUED -> JobState.QUEUED
                    KernelJobState.SENDING -> JobState.SENDING
                    KernelJobState.UNCONFIRMED -> JobState.UNCONFIRMED
                },
                job.attempts.toInt(),
            )
        }
    }

    override fun resolve(job: String, printed: Boolean) =
        call { register.resolve(job, if (printed) Resolution.PRINTED else Resolution.AGAIN) }.let { }

    override fun reprint(order: String) = call { register.reprint(order) }.let { }

    /** Joins the location's network with [kit]: replication runs from [syncLoop]. */
    fun startSync(kit: SyncKit) = call { register.startSync(kit) }

    /**
     * Runs replication until cancelled: a turn of it on the kernel's thread, [kernel], every
     * [every], each without waiting, so a cashier's intents wait for none of it.
     */
    suspend fun syncLoop(kernel: CoroutineDispatcher, every: Duration = 50.milliseconds) {
        while (true) {
            val failed = withContext(kernel) {
                try {
                    register.syncStep(0u)
                    false
                } catch (_: KeelException) {
                    // A frame that couldn't be stored: its peer sends it again.
                    true
                }
            }
            // A store that fails, fails again: wait longer before the next turn. Found in review:
            // a damaged store failed twenty times a second.
            delay(if (failed) every * 20 else every)
        }
    }

    override fun profileVersion(): String = register.profileVersion()

    override fun syncState(): SyncInfo? = call {
        register.syncStatus()?.let { SyncInfo(it.listening, it.settled, it.hubReachable, it.serving, it.profileVersion) }
    }

    /** The print driver's loop for this till's register, sending to the printer [address] gives. */
    fun printLoop(kernel: CoroutineDispatcher, address: () -> PrinterAddress?): PrintLoop =
        PrintLoop(register, kernel, address)

    override fun close() = register.close()
}

/** Runs [body], turning the kernel's refusals into the cashier's words. */
private fun <T> call(body: () -> T): T = try {
    body()
} catch (refused: KeelException) {
    val approval = (refused as? KeelException.ApprovalRequired)?.action
    throw TillException(words(refused), refused, approval)
}

/** What an action of ADR-0029 is, in words for the cashier. */
internal fun action(action: String): String = when (action) {
    "order.void" -> "Dropping an order with items on it"
    "line.remove" -> "Taking an item off"
    "discount.apply" -> "That discount"
    "price.override" -> "Changing a price"
    "refund.issue" -> "That refund"
    "drawer.no_sale" -> "Opening the drawer"
    "drawer.variance" -> "Closing the drawer over or short"
    "drawer.paid_out" -> "Paying out"
    "receipt.reprint" -> "Printing a receipt again"
    "item.availability" -> "Marking an item out"
    else -> "That"
}

/** What the cashier is told for [refused]. */
internal fun words(refused: KeelException): String = when (refused) {
    is KeelException.NotSignedIn -> "No one is signed in."
    is KeelException.NotSettled -> "Waiting for the store's hub: try again in a moment."
    is KeelException.UnknownMember -> "That team member isn't on this register."
    is KeelException.WrongPin -> "That PIN isn't right."
    is KeelException.NothingToApprove -> "Nothing is waiting for an approval."
    is KeelException.LockedOut -> "Too many wrong PINs: try again in ${refused.seconds} seconds."
    is KeelException.ApprovalRequired -> "${action(refused.action)} needs a manager's approval."
    is KeelException.NotApprover -> "That manager can't approve ${action(refused.action).lowercase()}."
    is KeelException.UnknownOrder -> "This register has no such order."
    is KeelException.NoReceipt -> "That order printed no receipt to print again."
    is KeelException.NotUnconfirmed -> "That job isn't waiting for a decision."
    is KeelException.UnknownJob -> "This register has no such print job."
    is KeelException.Catalog -> refused.reason
    is KeelException.Refused -> refused.reason
    is KeelException.TenderShort -> "That's short: ${refused.due.text} is due."
    is KeelException.WrongCurrency -> "That isn't this register's currency."
    is KeelException.NegativeTender -> "A tender can't be less than nothing."
    is KeelException.Damaged -> "This register's store is damaged. Call support."
    is KeelException.StoreInUse -> "This register's store is open already. Close the app and open it again."
    is KeelException.WrongKey -> "This register's key doesn't open its store. Call support."
    is KeelException.StoreNewer -> "This register's store is from a newer version of the app. Update the app."
    is KeelException.NotThisDevice -> "This register's store belongs to another device. Call support."
    is KeelException.InvalidArgument -> "Something went wrong: ${refused.reason}"
    is KeelException.Device -> "Something went wrong: ${refused.reason}"
}

private fun KernelAmount.toAmount() = Amount(minor, text)

private fun KernelDrawer.toDrawer() = Drawer(
    open = open,
    float = float.text,
    taken = taken.text,
    paidIn = paidIn.text,
    paidOut = paidOut.text,
    dropped = dropped.text,
    noSales = noSales.toLong(),
    expected = expected?.text,
    counted = counted?.text,
    variance = variance?.text,
)

private const val MILLION = 1_000_000L

private fun KernelLine.toLine() = TicketLine(
    line = line,
    name = name,
    quantity = quantityText,
    amount = amount.text,
    count = if (quantityMillionths % MILLION == 0L) (quantityMillionths / MILLION).toInt() else null,
    modifiers = modifiers.map(::words),
    discounts = discounts.map { it.toDiscount() },
    repriced = repriced?.let { "${it.unitPrice.text} (was ${it.rung.text}), ${reasonWords(it.reason).lowercase()}" },
    notes = notes,
)

private fun KernelCheck.toCheck() = TicketCheck(
    check = check,
    number = number.toInt(),
    open = state == TicketState.OPEN,
    lines = lines.map { it.toLine() },
    total = total.text,
    due = due.text,
    cashDueMinor = cashDue.minor,
    cashDue = cashDue.text,
)

private fun KernelTicket.toTicket() = Ticket(
    order = order,
    lines = lines.map { it.toLine() },
    checks = checks.map { it.toCheck() },
    owner = owner,
    asking = asking,
    discounts = discounts.map { it.toDiscount() },
    total = total.text,
    cashDueMinor = cashDue.minor,
    cashDue = cashDue.text,
    state = when (state) {
        TicketState.OPEN -> OrderState.OPEN
        TicketState.CLOSED -> OrderState.CLOSED
        TicketState.VOIDED -> OrderState.VOIDED
        TicketState.ABANDONED -> OrderState.ABANDONED
    },
    subtotal = subtotal.text,
    tax = taxes.takeIf { it.isNotEmpty() }?.joinToString { "${it.name} ${it.amount.text}" },
)

/** A discount in words, its reason and its percentage, and what it took: "Loyalty 10%", "−$2.10". */
private fun KernelDiscount.toDiscount() = TicketDiscount(
    discount = discount,
    words = reasonWords(reason) + (percentBasisPoints?.let { " " + percentWords(it.toInt()) } ?: ""),
    amount = "−" + amount.text,
)

/** A ticket's modifier in words, indented by its depth: "No onions", "Extra shot ×2 $1.90". */
private fun words(modifier: KernelModifier): String {
    val prefix = when (modifier.prefix) {
        Prefix.PLAIN -> ""
        Prefix.NO -> "No "
        Prefix.EXTRA -> "Extra "
        Prefix.LIGHT -> "Light "
        Prefix.SIDE -> "Side of "
        Prefix.SUB -> "Sub "
    }
    val times = if (modifier.quantity > 1u) " ×${modifier.quantity}" else ""
    val price = if (modifier.price.minor != 0L) " ${modifier.price.text}" else ""
    return "  ".repeat((modifier.depth.toInt() - 1).coerceAtLeast(0)) + prefix + modifier.name + times + price
}
