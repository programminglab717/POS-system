package dev.keel.android

import android.annotation.SuppressLint
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
import android.os.PowerManager

/**
 * Keeps a register joined to its location running in the foreground (ADR-0026, ADR-0027): its
 * replication, and the hub's role when it holds it, go on while the register's screen is closed.
 * Of type `connectedDevice`: it serves the store's other devices and its printer, on its network.
 * If the system stops the process, it starts the service again, which opens the kernel again; at
 * boot, [BootReceiver] starts it. It keeps the processor awake while it runs.
 */
class RegisterService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    // The wake lock is held for as long as the service runs, and released when it stops.
    @SuppressLint("WakelockTimeout")
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
        // The system may refuse the foreground, as when it restarts the service from the
        // background: the service stops, and the register's screen or the next boot starts it
        // again. Found in review: refused, it would have crashed the app.
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                startForeground(ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE)
            } else {
                startForeground(ID, notification)
            }
        } catch (_: RuntimeException) {
            stopSelf()
            return START_NOT_STICKY
        }
        // The hub answers the store's devices with the screen off: the processor stays awake while
        // the service runs, as a register on its stand is plugged in.
        if (wakeLock == null) {
            wakeLock = getSystemService(PowerManager::class.java)
                .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "keel:register")
                .apply { setReferenceCounted(false) }
        }
        wakeLock?.acquire()  // for the service's life: released in onDestroy
        (application as KeelApplication).host.start()
        return START_STICKY
    }

    override fun onDestroy() {
        wakeLock?.release()
        super.onDestroy()
    }

    private var wakeLock: PowerManager.WakeLock? = null

    companion object {
        private const val CHANNEL = "keel.register"
        private const val ID = 1

        /**
         * Starts the service for a provisioned register, from the register's screen while it is in
         * the foreground, or at boot, where the system allows it. Elsewhere the system may refuse,
         * which changes nothing here: the screen starts it next time it opens.
         */
        fun start(context: Context) {
            if (!(context.applicationContext as KeelApplication).host.provisioned) return
            try {
                context.startForegroundService(Intent(context, RegisterService::class.java))
            } catch (_: RuntimeException) {
                // Not allowed from the background, as on Android 12 and later.
            }
        }
    }
}
