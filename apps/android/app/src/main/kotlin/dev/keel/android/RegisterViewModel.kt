package dev.keel.android

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import dev.keel.register.RegisterModel
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlin.time.Duration.Companion.seconds

/** The register's state for the activity's life, over the kernel the process holds. */
class RegisterViewModel(application: Application) : AndroidViewModel(application) {
    private val host = (application as KeelApplication).host

    val model = RegisterModel(host.till, viewModelScope, host.kernel, host.printer)

    init {
        model.start()
        host.start()
        // How replication stands, for the screen, while it is open.
        viewModelScope.launch {
            while (true) {
                model.refreshSync()
                delay(2.seconds)
            }
        }
    }
}
