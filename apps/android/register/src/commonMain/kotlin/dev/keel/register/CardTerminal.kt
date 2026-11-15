package dev.keel.register

import kotlin.concurrent.Volatile
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * What the register asks a terminal (ADR-0035): a [Kind.SALE] or a [Kind.REFUND] to perform, or,
 * with [status], to say what became of one started before, which must never be performed again.
 * [key] is the idempotency key the terminal or processor sees.
 */
data class TerminalRequest(
    val key: String,
    val kind: Kind,
    val amountMinor: Long,
    /** The tip chosen on the register, on top of a sale's amount. */
    val tipMinor: Long? = null,
    /** The tip presets, in basis points, when the terminal asks for the tip. */
    val presets: List<Int> = emptyList(),
    /** Whether the terminal may approve less than asked. */
    val partial: Boolean = false,
    val status: Boolean = false,
    /** The processor's reference of the payment a refund goes back to. */
    val reference: String? = null,
) {
    enum class Kind { SALE, REFUND }

    /** What the terminal charges for a sale, or refunds: the amount and any tip. */
    val totalMinor: Long get() = amountMinor + (tipMinor ?: 0)
}

/** How a card was read. */
enum class CardEntry { TAP, CHIP, SWIPE, KEYED }

/** What a terminal tells of a card: what a receipt shows, never more. */
data class TerminalCard(val brand: String, val lastFour: String, val entry: CardEntry, val authCode: String? = null)

/** What a terminal answered. */
sealed interface TerminalOutcome {
    /** Approved for [approvedMinor], a tip included: less than asked is a partial approval. */
    data class Approved(val approvedMinor: Long, val reference: String?, val card: TerminalCard?) : TerminalOutcome

    /** Declined, for [reason], a reason code such as `declined` or `cancelled`. */
    data class Declined(val reason: String, val note: String? = null) : TerminalOutcome

    /** To a status query: the terminal never got the request. */
    data object NotFound : TerminalOutcome

    /** No one knows: the register asks again later. */
    data object Unknown : TerminalOutcome
}

/**
 * A card terminal, semi-integrated (I-02): card data never reaches the register. A connector for
 * a processor implements it once gate G2 chooses one; [SimulatedTerminal] stands in until then.
 */
interface CardTerminal {
    /** Whether it can ask the customer for a tip. */
    val asksTip: Boolean

    /** Whether it can approve less than asked. */
    val approvesPart: Boolean

    /**
     * Takes [request] and answers: performs a sale or a refund, or, for a status query, says
     * what it did with it. It may take as long as the customer does; [cancel] ends a wait.
     */
    suspend fun take(request: TerminalRequest): TerminalOutcome

    /**
     * Asks it to stop waiting for the card for the request with [key]: it answers that sale
     * declined, `cancelled`, unless the card was read first. A request it doesn't hold is left
     * alone. Found in review: a cancel with no key ended whatever the terminal held.
     */
    fun cancel(key: String)
}

/** What the simulated terminal does with the next request it is asked to perform. */
enum class SimulatedAnswer { APPROVE, APPROVE_PART, DECLINE, NO_ANSWER }

/** An answer given for the customer, or a cancel from the register. */
private sealed interface Given {
    data class Answer(val answer: SimulatedAnswer) : Given
    data object Cancelled : Given
}

/**
 * A terminal in software, for demonstrations and tests until a processor is chosen (ADR-0035): it
 * keeps a ledger by key, so a status query answers truthfully, and never performs a key twice.
 * Each sale it is asked to perform waits for [answer], as a customer at a terminal would,
 * unless [auto] answers it at once; a refund, by reference, needs no customer, and is approved at
 * once. Its ledger lasts as long as the process: after a restart it knows of nothing.
 */
class SimulatedTerminal(
    override val asksTip: Boolean = true,
    override val approvesPart: Boolean = true,
    /** The answer given at once to every request, or null to wait for [answer]. */
    private val auto: SimulatedAnswer? = null,
) : CardTerminal {
    private val ledger = mutableMapOf<String, TerminalOutcome>()

    /** The wait for the customer, written by the loop's thread and read by the screen's. */
    @Volatile
    private var waiting: Pair<String, CompletableDeferred<Given>>? = null

    /** Sales cancelled before the terminal took them: cancelled as they come. */
    @Volatile
    private var cancelled: Set<String> = emptySet()
    private val asked = MutableStateFlow<TerminalRequest?>(null)

    /** The request waiting for the customer, for the screen to show it. */
    val waitingFor: StateFlow<TerminalRequest?> = asked.asStateFlow()

    /** Answers the request waiting for the customer as [answer] says. */
    fun answer(answer: SimulatedAnswer) {
        waiting?.second?.complete(Given.Answer(answer))
    }

    override fun cancel(key: String) {
        val now = waiting
        if (now != null && now.first == key) {
            now.second.complete(Given.Cancelled)
        } else {
            // Not taken yet: cancelled as it comes. Found in review: a cancel before the loop
            // took the sale was lost, and the sale went on.
            cancelled = cancelled + key
        }
    }

    override suspend fun take(request: TerminalRequest): TerminalOutcome {
        // Asked about it: the ledger says, and nothing is performed.
        if (request.status) return ledger[request.key] ?: TerminalOutcome.NotFound
        ledger[request.key]?.let { return it }
        val early = request.key in cancelled
        if (early) cancelled = cancelled - request.key
        val answer = when {
            early -> null
            request.kind == TerminalRequest.Kind.REFUND -> SimulatedAnswer.APPROVE
            auto != null -> auto
            else -> {
                val wait = CompletableDeferred<Given>()
                waiting = request.key to wait
                asked.value = request
                // The loop's own cancellation passes on: it isn't the customer's.
                val given = try {
                    wait.await()
                } finally {
                    waiting = null
                    asked.value = null
                }
                (given as? Given.Answer)?.answer
            }
        }
        // Asked for the tip, the customer chooses the terminal's first preset.
        val tip = request.presets.firstOrNull()?.let { (request.amountMinor * it + 5_000) / 10_000 } ?: 0
        val outcome = when (answer) {
            null -> TerminalOutcome.Declined("cancelled")
            SimulatedAnswer.APPROVE -> approved(request.totalMinor + tip)
            SimulatedAnswer.APPROVE_PART ->
                if (request.partial && request.kind == TerminalRequest.Kind.SALE) {
                    approved((request.amountMinor / 2).coerceAtLeast(1))
                } else {
                    TerminalOutcome.Declined("insufficient_funds")
                }
            SimulatedAnswer.DECLINE -> TerminalOutcome.Declined("declined", "Declined by the simulated terminal")
            // The terminal charges, but the register never hears: it will ask.
            SimulatedAnswer.NO_ANSWER -> {
                ledger[request.key] = approved(request.totalMinor + tip)
                return TerminalOutcome.Unknown
            }
        }
        ledger[request.key] = outcome
        return outcome
    }

    private fun approved(minor: Long) = TerminalOutcome.Approved(
        approvedMinor = minor,
        reference = "sim_${ledger.size + 1}",
        card = TerminalCard("Visa", "4242", CardEntry.TAP, "SIM${ledger.size + 1}"),
    )
}
