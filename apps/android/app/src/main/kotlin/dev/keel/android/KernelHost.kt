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
