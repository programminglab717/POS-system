package dev.keel.android.kernel

import dev.keel.kernel.Choice
import dev.keel.kernel.KeelException
import dev.keel.kernel.Mode
import dev.keel.kernel.Prefix
import dev.keel.kernel.Register
import dev.keel.kernel.RegisterConfig
import dev.keel.kernel.TicketState
import dev.keel.register.Amount
import dev.keel.register.Chosen
import dev.keel.register.Group
import dev.keel.register.Item
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

/** The register's [Till], answered by the kernel's [register], which it closes with itself. */
class KernelTill(private val register: Register) : Till, AutoCloseable {
    companion object {
        /** The till over the register [config] opens, its refusals in the cashier's words. */
        fun open(config: RegisterConfig): KernelTill = call { KernelTill(Register.open(config)) }
    }

    override fun team(): List<Member> = call { register.team().map { Member(it.id, it.name) } }

    override fun signIn(member: String) = call { register.signIn(member) }

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

    override fun abandon(order: String): Ticket = call { register.abandon(order).toTicket() }

    override fun cashOffers(order: String): List<Amount> = call { register.cashOffers(order).map { it.toAmount() } }

    override fun amount(minor: Long): Amount = call { register.amount(minor).toAmount() }

    override fun payCash(order: String, tenderedMinor: Long): Paid = call {
        val paid = register.payCash(order, tenderedMinor)
        Paid(paid.change.text, paid.ticket.toTicket())
    }

    override fun ticket(order: String): Ticket = call { register.ticket(order).toTicket() }

    override fun ordersToday(): List<Ticket> = call { register.ordersToday().map { it.toTicket() } }

    override fun close() = register.close()
}

/** Runs [body], turning the kernel's refusals into the cashier's words. */
private fun <T> call(body: () -> T): T = try {
    body()
} catch (refused: KeelException) {
    throw TillException(words(refused), refused)
}

/** What the cashier is told for [refused]. */
internal fun words(refused: KeelException): String = when (refused) {
    is KeelException.NotSignedIn -> "No one is signed in."
    is KeelException.UnknownMember -> "That team member isn't on this register."
    is KeelException.UnknownOrder -> "This register has no such order."
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
    else -> "Something went wrong: ${refused::class.simpleName}."
}

private fun KernelAmount.toAmount() = Amount(minor, text)

private const val MILLION = 1_000_000L

private fun KernelTicket.toTicket() = Ticket(
    order = order,
    lines = lines.map { line ->
        TicketLine(
            line = line.line,
            name = line.name,
            quantity = line.quantityText,
            amount = line.amount.text,
            count = if (line.quantityMillionths % MILLION == 0L) (line.quantityMillionths / MILLION).toInt() else null,
            modifiers = line.modifiers.map(::words),
        )
    },
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
