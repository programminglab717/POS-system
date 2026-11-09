package dev.keel.android.kernel

import dev.keel.kernel.KeelException
import dev.keel.kernel.Mode
import dev.keel.kernel.Register
import dev.keel.register.MenuButton
import dev.keel.register.MenuPage
import dev.keel.register.Paid
import dev.keel.register.Ticket
import dev.keel.register.TicketLine
import dev.keel.register.Till
import dev.keel.register.TillException
import dev.keel.kernel.Ticket as KernelTicket

/** The register's [Till], answered by the kernel's [register], which it closes with itself. */
class KernelTill(private val register: Register) : Till, AutoCloseable {
    override fun signIn(member: String) = call { register.signIn(member) }

    override fun menu(): List<MenuPage> = call {
        register.menu().pages.map { page ->
            MenuPage(page.name, page.buttons.map { MenuButton(it.variant, it.name, it.price.text) })
        }
    }

    override fun startOrder(): Ticket = call { register.startOrder(Mode.TAKEOUT).toTicket() }

    override fun addItem(order: String, variant: String): Ticket =
        call { register.addItem(order, variant, emptyList(), 1u).toTicket() }

    override fun payCash(order: String, tenderedMinor: Long): Paid = call {
        val paid = register.payCash(order, tenderedMinor)
        Paid(paid.change.text, paid.ticket.toTicket())
    }

    override fun close() = register.close()

    /** Runs [body], turning the kernel's refusals into the cashier's words. */
    private fun <T> call(body: () -> T): T = try {
        body()
    } catch (refused: KeelException) {
        throw TillException(words(refused), refused)
    }
}

/** What the cashier is told for [refused]. */
internal fun words(refused: KeelException): String = when (refused) {
    is KeelException.NotSignedIn -> "No one is signed in."
    is KeelException.UnknownMember -> "That team member isn't on this register."
    is KeelException.Catalog -> refused.reason
    is KeelException.Refused -> refused.reason
    is KeelException.TenderShort -> "That's short: ${refused.due.text} is due."
    is KeelException.NegativeTender -> "A tender can't be less than nothing."
    is KeelException.Damaged -> "This register's store is damaged. Call support."
    else -> "Something went wrong: ${refused::class.simpleName}."
}

private fun KernelTicket.toTicket() = Ticket(
    order = order,
    lines = lines.map { TicketLine(it.line, it.name, it.quantityText, it.amount.text) },
    total = total.text,
    cashDueMinor = cashDue.minor,
    cashDue = cashDue.text,
)
