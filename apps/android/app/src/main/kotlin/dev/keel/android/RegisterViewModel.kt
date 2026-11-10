package dev.keel.android

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import dev.keel.android.kernel.DeviceFiles
import dev.keel.android.kernel.KernelTill
import dev.keel.kernel.RegisterConfig
import dev.keel.register.RegisterModel
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File
import java.util.concurrent.Executors
import kotlin.time.Duration.Companion.seconds

/**
 * The register for the activity's life: the kernel opened on a thread of its own, the one that
 * makes every call to it, and the register's state over it.
 */
class RegisterViewModel(application: Application) : AndroidViewModel(application) {
    private val executor = Executors.newSingleThreadExecutor { runnable -> Thread(runnable, "keel-kernel") }
    private val kernel = executor.asCoroutineDispatcher()
    private val till = OpeningTill { open(application) }
    private val printer = StoredPrinter(application)

    val model = RegisterModel(till, viewModelScope, kernel, printer)

    init {
        model.start()
        // Once the kernel is open, the print driver and, for a register joined to its location,
        // replication; if it can't open, the register says why.
        viewModelScope.launch(Dispatchers.IO) {
            val opened = try {
                withContext(kernel) { till.open() } as? KernelTill
            } catch (_: Exception) {
                null
            } ?: return@launch
            launch { opened.syncLoop(kernel) }
            launch {
                while (true) {
                    model.refreshSync()
                    delay(2.seconds)
                }
            }
            opened.printLoop(kernel) { model.state.value.printer }.run()
        }
    }

    override fun onCleared() {
        executor.execute { (till.openedTill as? AutoCloseable)?.close() }
        executor.shutdown()
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
