package dev.keel.android

import android.app.Application
import dev.keel.android.kernel.DeviceFiles
import dev.keel.android.kernel.KernelTill
import dev.keel.kernel.RegisterConfig
import dev.keel.kernel.SyncKit
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.supervisorScope
import kotlinx.coroutines.withContext
import java.io.File
import java.util.concurrent.Executors
import kotlin.time.Duration
import kotlin.time.Duration.Companion.minutes
import kotlin.time.Duration.Companion.seconds

/**
 * The kernel for the process's life (ADR-0027): opened on a thread of its own, the one that makes
 * every call to it, with the print driver and, for a provisioned register, replication with its
 * location, which run whether or not the register's screen is open. [RegisterService] keeps the
 * process in the foreground while a provisioned register runs.
 */
class KernelHost(private val application: Application) {
    private val executor = Executors.newSingleThreadExecutor { runnable -> Thread(runnable, "keel-kernel") }

    /** The kernel's thread: every call to the kernel is made on it. */
    val kernel = executor.asCoroutineDispatcher()

    /** The register's till, opened on the kernel's thread at its first call. */
    val till = OpeningTill { open() }

    /** Where the printer listens. */
    val printer = StoredPrinter(application)

    /**
     * The register's card terminal: simulated until gate G2 chooses the first processor, whose
     * connector takes its place (ADR-0035). The screen answers for the customer.
     */
    val terminal = dev.keel.register.SimulatedTerminal()

    private val dir = File(application.noBackupFilesDir, "keel")
    private val kit = application.getExternalFilesDir(null)?.resolve("kit")

    /** What the register joins its location with, once it is open; set on the kernel's thread. */
    private var sync: SyncKit? = null

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private var running: Job? = null

    /**
     * Whether this register is provisioned to join its location, or will be when it opens: a
     * roster among its files, or a kit waiting to be imported. Read on any thread.
     */
    val provisioned: Boolean
        get() = File(dir, "roster").isFile || kit?.resolve("roster")?.isFile == true

    /**
     * Starts the print driver and, for a provisioned register, replication, once the kernel is
     * open. Called again, it does nothing while they run; after a kernel that wouldn't open, it
     * tries again. Found in review: a first open that failed left them stopped for the process's
     * life, though a later open succeeded.
     */
    @Synchronized
    fun start() {
        if (running?.isActive == true) return
        running = scope.launch {
            val opened = try {
                withContext(kernel) { till.open() } as? KernelTill
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (_: Exception) {
                // The register's screen says why, from its own call; the next start tries again.
                null
            } ?: return@launch
            val joining = withContext(kernel) { sync }
            // Each runs on whatever the other does: a print loop that fails stops no replication.
            supervisorScope {
                if (joining != null) {
                    launch {
                        join(opened, joining)
                        opened.syncLoop(kernel)
                    }
                }
                launch { opened.printLoop(kernel) { printer.load() }.run() }
                launch { opened.paymentLoop(kernel, terminal).run() }
            }
        }
    }

    /**
     * Joins the location with [kit], trying again with a growing wait while it can't: a port
     * taken, or no network yet. The register sells meanwhile, as an island. Found in review: the
     * join ran as the kernel opened, and a join that failed left the store open and locked.
     */
    private suspend fun join(till: KernelTill, kit: SyncKit) {
        var wait: Duration = 5.seconds
        while (true) {
            val joined = withContext(kernel) {
                try {
                    till.startSync(kit)
                    true
                } catch (_: Exception) {
                    false
                }
            }
            if (joined) return
            delay(wait)
            wait = (wait * 2).coerceAtMost(5.minutes)
        }
    }

    /** The training register's card terminal: always simulated, never a real one (ADR-0038). */
    val trainingTerminal = dev.keel.register.SimulatedTerminal()

    /**
     * Opens a training register over a store of its own, on the kernel's thread (ADR-0038): a new
     * one each time, the last session's gone, so training starts from nothing. It never syncs, and
     * what it prints says so.
     */
    fun openTraining(): KernelTill {
        // The profile the real register rings from now, its latest publication: found in review,
        // the one provisioning wrote had none of the team or prices since.
        val real = till.open() as KernelTill
        val profile = real.profile()
        // An identity of its own, which no roster has: nothing it signs could be taken as the real
        // device's, nor fork its log. Found in review.
        val identity = dev.keel.kernel.newDevice()
        val training = File(dir, "training").apply { mkdirs() }
        training.listFiles()?.forEach { it.deleteRecursively() }
        return KernelTill.open(
            RegisterConfig(
                path = File(training, "store-${System.currentTimeMillis()}.db").path,
                storeKey = identity.storeKey,
                device = identity.device,
                signingKey = identity.signingKey,
                profile = profile,
                locale = "en-US",
                training = true,
            ),
        )
    }

    /** Closes a training register on the kernel's thread, once its loops are stopped. */
    fun closeTraining(till: KernelTill) {
        scope.launch {
            withContext(kernel) {
                try {
                    till.close()
                } catch (_: Exception) {
                    // Closed or not, its store is deleted when the next training opens.
                }
            }
        }
    }

    /** Runs a training register's print driver and payment loop, until the job is cancelled. */
    fun runTraining(till: KernelTill): Job = scope.launch {
        supervisorScope {
            launch { till.printLoop(kernel) { printer.load() }.run() }
            launch { till.paymentLoop(kernel, trainingTerminal).run() }
        }
    }

    /** Opens the kernel, on the kernel's thread: its identity, imported from a kit the first time. */
    private fun open(): KernelTill {
        // Kept out of backups, as the store and the keys must be. A provisioned kit, pushed to the
        // app's own external files, is imported once, then gone.
        val files = DeviceFiles.load(dir, KeyVault(), kit)
        val till = KernelTill.open(
            RegisterConfig(
                path = File(dir, "store.db").path,
                storeKey = files.storeKey,
                device = files.device,
                signingKey = files.signingKey,
                profile = files.profile,
                // The demo café's locale, until the profile names the location's.
                locale = "en-US",
            ),
        )
        sync = files.sync
        return till
    }
}
