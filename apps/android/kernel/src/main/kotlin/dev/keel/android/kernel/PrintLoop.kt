package dev.keel.android.kernel

import dev.keel.kernel.Confirm
import dev.keel.kernel.NotSent
import dev.keel.kernel.Outcome
import dev.keel.kernel.PrintJob
import dev.keel.kernel.Unconfirmed
import kotlinx.coroutines.CancellationException
import dev.keel.kernel.Printer
import dev.keel.kernel.Register
import dev.keel.kernel.Waits
import dev.keel.kernel.printTcp
import dev.keel.register.PrinterAddress
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import kotlin.time.Duration
import kotlin.time.Duration.Companion.seconds

/**
 * The print driver's loop (ADR-0024, ADR-0027): it takes each print job that is due from
 * [register], on the kernel's own thread, [kernel], so the kernel keeps one caller; sends it to
 * the printer [address] gives with `printTcp`, on the thread that runs [run], outside the
 * register's lock; and records what happened, on the kernel's thread again. A job the printer
 * may have printed in part is left unconfirmed, for staff to decide: the kernel never sends one
 * twice by itself.
 */
class PrintLoop(
    private val register: Register,
    private val kernel: CoroutineDispatcher,
    private val address: () -> PrinterAddress?,
    private val printer: Printer = Printer(columns = 48u, cutter = true, confirm = Confirm.PRINTED),
    private val waits: Waits = Waits(statusMs = 2_000u, wordMs = 10_000u),
    private val idle: Duration = 1.seconds,
    /** What sends a job to a printer: `printTcp`, but for tests. */
    private val send: (host: String, port: UShort, job: PrintJob, waits: Waits) -> Outcome = ::printTcp,
) {
    /**
     * Runs until cancelled: call it on a thread that may block, such as `Dispatchers.IO`. It asks
     * for jobs whether or not the register's printer is set: a kitchen ticket names its own
     * printer, and the kernel reroutes the tickets no screen showed as it is asked (ADR-0034).
     */
    suspend fun run() {
        var profiled = false
        while (true) {
            val target = address()
            val job = try {
                withContext(kernel) {
                    // Jobs for the register's own printer wait for its profile, set once there is
                    // an address to send them to.
                    if (target != null && !profiled) {
                        register.setPrinter(printer)
                        profiled = true
                    }
                    register.nextPrintJob()
                }
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (_: Exception) {
                null
            }
            if (job == null) {
                delay(idle)
                continue
            }
            // A kitchen job goes to the printer it names; another, to the register's own.
            val host = job.host ?: target?.host
            val port = job.port ?: target?.port?.toUShort()
            val outcome = if (host == null || port == null) {
                Outcome.NotSent(NotSent.UNREACHABLE)
            } else {
                try {
                    send(host, port, job, waits)
                } catch (cancelled: CancellationException) {
                    throw cancelled
                } catch (_: Exception) {
                    // A job the driver refuses, as none the register gives is, or a failure in
                    // it: whether any of it reached the printer is unknown, so staff decide.
                    // Found in review: the job stayed with the driver, and nothing printed after.
                    Outcome.Unconfirmed(Unconfirmed.SEND_FAILED)
                }
            }
            try {
                withContext(kernel) { register.printOutcome(job.job, outcome) }
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (_: Exception) {
                // The store couldn't record it: the job stays with the driver until the register
                // opens again, which leaves it unconfirmed, for staff.
                delay(idle)
            }
        }
    }
}
