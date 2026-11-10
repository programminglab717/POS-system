package dev.keel.android

import android.app.Application
import dev.keel.android.kernel.DeviceFiles
import dev.keel.android.kernel.KernelTill
import dev.keel.kernel.RegisterConfig
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File
import java.util.concurrent.Executors

/**
 * The kernel for the process's life (ADR-0027): opened on a thread of its own, the one that makes
 * every call to it, with the print driver and, for a register joined to its location,
 * replication, which run whether or not the register's screen is open. [RegisterService] keeps
 * the process in the foreground while a joined register runs.
 */
class KernelHost(private val application: Application) {
    private val executor = Executors.newSingleThreadExecutor { runnable -> Thread(runnable, "keel-kernel") }

    /** The kernel's thread: every call to the kernel is made on it. */
    val kernel = executor.asCoroutineDispatcher()

    /** The register's till, opened on the kernel's thread at its first call. */
    val till = OpeningTill { open(application) }

    /** Where the printer listens. */
    val printer = StoredPrinter(application)

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private var started = false

    /**
     * Starts, once, the print driver and replication when the kernel is open; and the foreground
     * service, for a register joined to its location. If the kernel can't open, the register's
     * screen says why.
     */
    @Synchronized
    fun start() {
        if (started) return
        started = true
        scope.launch {
            val opened = try {
                withContext(kernel) { till.open() } as? KernelTill
            } catch (_: Exception) {
                null
            } ?: return@launch
            val joined = withContext(kernel) { runCatching { opened.syncState() }.getOrNull() != null }
            if (joined) {
                RegisterService.start(application)
                launch { opened.syncLoop(kernel) }
            }
            opened.printLoop(kernel) { printer.load() }.run()
        }
    }

    private companion object {
        fun open(application: Application): KernelTill {
            // Kept out of backups, as the store and the keys must be.
            val dir = File(application.noBackupFilesDir, "keel")
            // A provisioned kit, pushed to the app's own external files: imported once, then gone.
            val kit = application.getExternalFilesDir(null)?.resolve("kit")
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
            files.sync?.let(till::startSync)
            return till
        }
    }
}
