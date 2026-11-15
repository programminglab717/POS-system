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

    val model = RegisterModel(host.till, viewModelScope, host.kernel, host.printer, StoredStations(application), host.terminal)

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
        // A card payment open is read every second, as the terminal answers it; the card refunds
        // owed, every ten (ADR-0035).
        viewModelScope.launch {
            var ticks = 0
            while (true) {
                val payment = model.state.value.card?.payment
                if (payment != null) model.refreshCard()
                if (ticks % 10 == 0 && model.state.value.member != null) model.refreshOwedRefunds()
                ticks += 1
                delay(1.seconds)
            }
        }
        // A kitchen screen reads its tickets every second, as they age (ADR-0034).
        viewModelScope.launch {
            while (true) {
                if (model.state.value.kds != null) model.refreshKitchen()
                delay(1.seconds)
            }
        }
    }
}
