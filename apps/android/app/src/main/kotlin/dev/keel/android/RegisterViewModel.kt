package dev.keel.android

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import dev.keel.android.kernel.KernelTill
import dev.keel.kernel.Register
import dev.keel.kernel.RegisterConfig
import dev.keel.kernel.demoProfile
import dev.keel.register.RegisterModel
import dev.keel.register.Till
import kotlinx.coroutines.asCoroutineDispatcher
import java.io.File
import java.util.concurrent.Executors

/**
 * The register for the activity's life: the kernel opened on a thread of its own, the one that
 * makes every call to it, and the register's state over it.
 */
class RegisterViewModel(application: Application) : AndroidViewModel(application) {
    private val executor = Executors.newSingleThreadExecutor { runnable -> Thread(runnable, "keel-kernel") }
    private val opened = lazy { open(application) }

    /** The till, opened on the kernel's thread at its first call. */
    private val till = object : Till {
        private val kernel get() = opened.value
        override fun signIn(member: String) = kernel.signIn(member)
        override fun menu() = kernel.menu()
        override fun startOrder() = kernel.startOrder()
        override fun addItem(order: String, variant: String) = kernel.addItem(order, variant)
        override fun payCash(order: String, tenderedMinor: Long) = kernel.payCash(order, tenderedMinor)
    }

    val model = RegisterModel(till, viewModelScope, executor.asCoroutineDispatcher())

    init {
        // The demo café's first team member, until the sign-in screen lists the team.
        model.start(DEMO_MEMBER)
    }

    override fun onCleared() {
        executor.execute { if (opened.isInitialized()) opened.value.close() }
        executor.shutdown()
    }

    private companion object {
        const val DEMO_MEMBER = "0192f0c1-0000-7000-8000-000000000501"

        fun open(application: Application): KernelTill {
            // Kept out of backups, as the store and the keys must be.
            val dir = File(application.noBackupFilesDir, "keel")
            val identity = DeviceIdentity.load(dir, KeyVault())
            val register = Register.open(
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
            return KernelTill(register)
        }
    }
}
