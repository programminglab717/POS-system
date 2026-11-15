package dev.keel.android.kernel

import dev.keel.kernel.CardAsk
import dev.keel.kernel.CardEntry as KernelCardEntry
import dev.keel.kernel.CardOutcome
import dev.keel.kernel.CardRead
import dev.keel.kernel.CardRequest
import dev.keel.kernel.Register
import dev.keel.register.CardEntry
import dev.keel.register.CardTerminal
import dev.keel.register.TerminalOutcome
import dev.keel.register.TerminalRequest
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import kotlin.time.Duration
import kotlin.time.Duration.Companion.seconds

/**
 * The card terminal's loop (ADR-0035), as the print loop is the printer's: it takes each card
 * request that is due from [register], on the kernel's own thread, [kernel]; takes it to the
 * [terminal], on the thread that runs [run], outside the register's lock, for as long as the
 * customer takes; and records the answer, on the kernel's thread again. A request the kernel
 * marks as a status query is only ever asked about: the kernel never has a card charged twice.
 */
class PaymentLoop(
    private val register: Register,
    private val kernel: CoroutineDispatcher,
    private val terminal: CardTerminal,
    private val idle: Duration = 1.seconds,
) {
    /** Runs until cancelled: call it on a thread that may block, such as `Dispatchers.IO`. */
    suspend fun run() {
        while (true) {
            val request = try {
                withContext(kernel) { register.nextCardRequest() }
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (_: Exception) {
                null
            }
            if (request == null) {
                delay(idle)
                continue
            }
            val outcome = try {
                terminal.take(request.toTerminal()).toKernel()
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (_: Exception) {
                // What the terminal did is unknown: the kernel asks it later, and never sends it again.
                CardOutcome.Unknown(null)
            }
            try {
                withContext(kernel) { register.cardOutcome(request.key, outcome) }
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (_: Exception) {
                // Not recorded: the request is in doubt, and the kernel asks about it next.
                delay(idle)
            }
        }
    }
}

/** The kernel's request, as a terminal takes it. */
internal fun CardRequest.toTerminal(): TerminalRequest = when (val ask = ask) {
    is CardAsk.Sale -> TerminalRequest(
        key = key,
        kind = TerminalRequest.Kind.SALE,
        amountMinor = ask.amount,
        tipMinor = ask.tip,
        presets = ask.presets.map { it.toInt() },
        partial = ask.partial,
        status = status,
    )
    is CardAsk.Refund -> TerminalRequest(
        key = key,
        kind = TerminalRequest.Kind.REFUND,
        amountMinor = ask.amount,
        status = status,
        reference = ask.reference,
    )
}

/** A terminal's answer, as the kernel records it. */
internal fun TerminalOutcome.toKernel(): CardOutcome = when (this) {
    is TerminalOutcome.Approved -> CardOutcome.Approved(
        approved = approvedMinor,
        reference = reference,
        card = card?.let { card ->
            CardRead(
                brand = card.brand,
                lastFour = card.lastFour,
                entry = when (card.entry) {
                    CardEntry.TAP -> KernelCardEntry.TAP
                    CardEntry.CHIP -> KernelCardEntry.CHIP
                    CardEntry.SWIPE -> KernelCardEntry.SWIPE
                    CardEntry.KEYED -> KernelCardEntry.KEYED
                },
                authCode = card.authCode,
            )
        },
    )
    is TerminalOutcome.Declined -> CardOutcome.Declined(reason, note, null)
    TerminalOutcome.NotFound -> CardOutcome.NotFound
    TerminalOutcome.Unknown -> CardOutcome.Unknown(null)
}
