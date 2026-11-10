package dev.keel.android

import android.content.Context
import dev.keel.register.PrinterAddress
import dev.keel.register.PrinterSettings

/** Where the printer listens, kept in the app's preferences: nothing secret, and never backed up. */
class StoredPrinter(context: Context) : PrinterSettings {
    private val preferences = context.getSharedPreferences("keel.printer", Context.MODE_PRIVATE)

    override fun load(): PrinterAddress? {
        val host = preferences.getString(HOST, null) ?: return null
        return PrinterAddress(host, preferences.getInt(PORT, 9100))
    }

    override fun save(address: PrinterAddress) {
        preferences.edit().putString(HOST, address.host).putInt(PORT, address.port).apply()
    }

    private companion object {
        const val HOST = "host"
        const val PORT = "port"
    }
}
