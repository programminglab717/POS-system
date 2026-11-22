package dev.keel.android

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import dev.keel.android.kernel.KernelTill
import dev.keel.register.RegisterModel
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.withContext
import kotlinx.coroutines.launch
import kotlin.time.Duration.Companion.seconds

/** The register's state for the activity's life, over the kernel the process holds. */
class RegisterViewModel(application: Application) : AndroidViewModel(application) {
    private val host = (application as KeelApplication).host

    val model = RegisterModel(host.till, viewModelScope, host.kernel, host.printer, StoredStations(application), host.terminal)

    /** The training register, while one is open (ADR-0038): the screen shows it in the real one's place. */
    val training = MutableStateFlow<RegisterModel?>(null)
    private var trainingTill: KernelTill? = null
    private var trainingLoops: Job? = null
    private var starting = false

    /** The register on the screen: training, or the real one. */
    private fun active(): RegisterModel = training.value ?: model

    /**
     * Opens a training register, over a new store of its own, and shows it. Once: a second tap while
     * it opens does nothing. Found in review: two opened, and the first's loops ran on for good.
     */
    fun startTraining() {
        if (training.value != null || starting) return
        starting = true
        viewModelScope.launch {
            try {
                val till = withContext(host.kernel) { host.openTraining() }
                trainingTill = till
                val practice = RegisterModel(
                    till, viewModelScope, host.kernel, FrozenPrinter(host.printer),
                    FrozenStations(StoredStations(getApplication())), host.trainingTerminal,
                )
                practice.start()
                trainingLoops = host.runTraining(till)
                training.value = practice
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (failed: Exception) {
                // Said, not swallowed: found in review, the button did nothing.
                model.tell("Training couldn't start: ${failed.message ?: failed::class.simpleName}")
            } finally {
                starting = false
            }
        }
    }

    /** Ends training: the real register again, the training register's loops stopped and its store closed. */
    fun endTraining() {
        trainingLoops?.cancel()
        trainingLoops = null
        training.value = null
        val till = trainingTill ?: return
        trainingTill = null
        host.closeTraining(till)
    }

    override fun onCleared() {
        // Training doesn't outlive its screen: found in review, its loops ran on in the process.
        endTraining()
        super.onCleared()
    }

    init {
        model.start()
        host.start()
        // How replication stands, for the screen, while it is open.
        viewModelScope.launch {
            while (true) {
                active().refreshSync()
                delay(2.seconds)
            }
        }
        // A card payment open is read every second, as the terminal answers it; the card refunds
        // owed, every ten (ADR-0035).
        viewModelScope.launch {
            var ticks = 0
            while (true) {
                val shown = active()
                val payment = shown.state.value.card?.payment
                if (payment != null) shown.refreshCard()
                if (ticks % 10 == 0 && shown.state.value.member != null) shown.refreshOwedRefunds()
                ticks += 1
                delay(1.seconds)
            }
        }
        // A kitchen screen reads its tickets every second, as they age (ADR-0034).
        viewModelScope.launch {
            while (true) {
                val shown = active()
                if (shown.state.value.kds != null) shown.refreshKitchen()
                delay(1.seconds)
            }
        }
    }
}
