package dev.keel.android

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

/**
 * Starts a provisioned register's service when the device boots, so the store's hub is back
 * after a power cut without anyone opening the app. Found in review: it waited for the app.
 */
class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action == Intent.ACTION_BOOT_COMPLETED) RegisterService.start(context)
    }
}
