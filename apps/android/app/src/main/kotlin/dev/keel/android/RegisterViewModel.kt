package dev.keel.android

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import dev.keel.android.kernel.KernelTill
import dev.keel.kernel.RegisterConfig
import dev.keel.kernel.demoProfile
import dev.keel.register.RegisterModel
import kotlinx.coroutines.asCoroutineDispatcher
import java.io.File
import java.util.concurrent.Executors

/**
 * The register for the activity's life: the kernel opened on a thread of its own, the one that
 * makes every call to it, and the register's state over it.
 */
class RegisterViewModel(application: Application) : AndroidViewModel(application) {
    private val executor = Executors.newSingleThreadExecutor { runnable -> Thread(runnable, "keel-kernel") }
    private val till = OpeningTill { open(application) }

    val model = RegisterModel(till, viewModelScope, executor.asCoroutineDispatcher())

    init {
        model.start()
    }

    override fun onCleared() {
        executor.execute { (till.openedTill as? AutoCloseable)?.close() }
        executor.shutdown()
    }

    private companion object {
        fun open(application: Application): KernelTill {
            // Kept out of backups, as the store and the keys must be.
            val dir = File(application.noBackupFilesDir, "keel")
            val identity = DeviceIdentity.load(dir, KeyVault())
            return KernelTill.open(
                RegisterConfig(
                    path = File(dir, "store.db").path,
                    storeKey = identity.storeKey,
                    device = identity.device,
                    signingKey = identity.signingKey,
                    profile = demoProfile(),
                    // The demo café's locale, until the profile names the location's.
                    locale = "en-US",
                ),
            )
        }
    }
}
