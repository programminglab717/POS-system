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

/** What the register's screen shows. */
data class RegisterState(
    val loading: Boolean = true,
    val pages: List<MenuPage> = emptyList(),
    val page: Int = 0,
    val ticket: Ticket? = null,
    /** The change handed back for the last order paid, until the next is started. */
    val change: String? = null,
    /** The last refusal, until the next intent. */
    val error: String? = null,
)

/**
 * The register's state: the menu, the order being rung, and what the cashier last did. Each
 * intent runs on [worker], one at a time, so the kernel has one caller; the state changes when
 * it answers. A refusal, or any other failure, is kept in the state for the screen to show.
 */
class RegisterModel(
    private val till: Till,
    private val scope: CoroutineScope,
    private val worker: CoroutineDispatcher,
) {
    private val mutable = MutableStateFlow(RegisterState())

    /** What the screen shows now. */
    val state: StateFlow<RegisterState> = mutable.asStateFlow()

    /** Signs [member] in and loads the menu. */
    fun start(member: String) = perform {
        till.signIn(member)
        val pages = till.menu()
        mutable.update { it.copy(loading = false, pages = pages) }
    }

    /** Shows the menu's page [index]. */
    fun showPage(index: Int) {
        mutable.update { state -> state.copy(page = index.coerceIn(0, (state.pages.size - 1).coerceAtLeast(0))) }
    }

    /** Rings [variant], on a new order if none is open. */
    fun tap(variant: String) = perform {
        // An order started is shown at once, so a line it refuses doesn't leave it behind.
        val order = mutable.value.ticket?.order
            ?: till.startOrder().also { started -> mutable.update { it.copy(ticket = started) } }.order
        val ticket = till.addItem(order, variant)
        mutable.update { it.copy(ticket = ticket, change = null) }
    }

    /** Pays the open order with exactly what it owes in cash. */
    fun payExact() = perform {
        val ticket = mutable.value.ticket ?: return@perform
        val paid = till.payCash(ticket.order, ticket.cashDueMinor)
        mutable.update { it.copy(ticket = null, change = paid.change) }
    }

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
                    it.copy(loading = false, error = "Something went wrong: ${failed.message ?: failed::class.simpleName}")
                }
            }
        }
    }
}
