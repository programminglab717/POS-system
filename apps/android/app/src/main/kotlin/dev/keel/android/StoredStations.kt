package dev.keel.android

import android.content.Context
import dev.keel.register.KitchenSettings

/**
 * The kitchen stations this device's screen shows (ADR-0034), kept in the app's preferences:
 * which stations a screen shows is the device's setting, as its printer is.
 */
class StoredStations(context: Context) : KitchenSettings {
    private val preferences = context.getSharedPreferences("keel.kitchen", Context.MODE_PRIVATE)

    override fun load(): Set<String>? = preferences.getStringSet(STATIONS, null)?.toSet()?.takeIf { it.isNotEmpty() }

    override fun save(stations: Set<String>) {
        preferences.edit().putStringSet(STATIONS, stations.toSet()).apply()
    }

    private companion object {
        const val STATIONS = "stations"
    }
}
