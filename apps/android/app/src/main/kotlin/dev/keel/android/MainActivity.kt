package dev.keel.android

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.viewModels
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue

/** The register's one activity. */
class MainActivity : ComponentActivity() {
    private val register: RegisterViewModel by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            MaterialTheme {
                val training by register.training.collectAsState()
                RegisterScreen(
                    training ?: register.model,
                    Training(start = register::startTraining, end = register::endTraining, active = training != null),
                )
            }
        }
    }

    override fun onStart() {
        super.onStart()
        // In the foreground, where the system allows a foreground service to start.
        RegisterService.start(this)
    }
}
