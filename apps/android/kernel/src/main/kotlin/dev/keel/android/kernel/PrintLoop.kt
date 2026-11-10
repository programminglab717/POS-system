package dev.keel.android.kernel

import dev.keel.kernel.Confirm
import dev.keel.kernel.KeelException
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
) {
    /** Runs until cancelled: call it on a thread that may block, such as `Dispatchers.IO`. */
    suspend fun run() {
        var profiled = false
        while (true) {
            val target = address()
            if (target == null) {
                delay(idle)
                continue
            }
            val job = try {
                withContext(kernel) {
                    // Jobs wait for a printer's profile, to be rendered for it.
                    if (!profiled) register.setPrinter(printer)
                    profiled = true
                    register.nextPrintJob()
                }
            } catch (_: KeelException) {
                null
            }
            if (job == null) {
                delay(idle)
                continue
            }
            val outcome = try {
                printTcp(target.host, target.port.toUShort(), job, waits)
            } catch (_: KeelException) {
                // A job the driver refuses, as none the register gives is: it stays with the
                // driver until the register opens again, which leaves it unconfirmed, for staff.
                delay(idle)
                continue
            }
            try {
                withContext(kernel) { register.printOutcome(job.job, outcome) }
            } catch (_: KeelException) {
                // The store couldn't record it: the job stays with the driver until the register
                // opens again, which leaves it unconfirmed, for staff.
                delay(idle)
            }
        }
    }
}
