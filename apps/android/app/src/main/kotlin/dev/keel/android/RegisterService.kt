package dev.keel.android

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder

/**
 * Keeps a register joined to its location running in the foreground (ADR-0026, ADR-0027): its
 * replication, and the hub's role when it holds it, go on while the register's screen is closed.
 * Of type `connectedDevice`: it serves the store's other devices and its printer, on its network.
 * If the system stops the process, it starts the service again, which opens the kernel again.
 */
class RegisterService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val manager = getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL, getString(R.string.service_channel), NotificationManager.IMPORTANCE_LOW),
        )
        val open = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE,
        )
        val notification = Notification.Builder(this, CHANNEL)
            .setContentTitle(getString(R.string.app_name))
            .setContentText(getString(R.string.service_text))
            .setSmallIcon(android.R.drawable.stat_notify_sync)
            .setContentIntent(open)
            .setOngoing(true)
            .build()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            startForeground(ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE)
        } else {
            startForeground(ID, notification)
        }
        (application as KeelApplication).host.start()
        return START_STICKY
    }

    companion object {
        private const val CHANNEL = "keel.register"
        private const val ID = 1

        /** Starts the service, from the register's screen. */
        fun start(context: Context) {
            context.startForegroundService(Intent(context, RegisterService::class.java))
        }
    }
}
