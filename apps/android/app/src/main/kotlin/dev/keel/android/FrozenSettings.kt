package dev.keel.android

import dev.keel.register.KitchenSettings
import dev.keel.register.PrinterAddress
import dev.keel.register.PrinterSettings

/**
 * The real register's printer, as a training register reads it: what a trainee saves isn't kept,
 * so training never changes where real receipts print (ADR-0038). Found in review.
 */
class FrozenPrinter(private val real: PrinterSettings) : PrinterSettings {
    private var practised: PrinterAddress? = null

    override fun load(): PrinterAddress? = practised ?: real.load()

    override fun save(address: PrinterAddress) {
        practised = address
    }
}

/** The real register's kitchen stations, as a training register reads them: kept apart, as [FrozenPrinter]. */
class FrozenStations(private val real: KitchenSettings) : KitchenSettings {
    private var practised: Set<String>? = null

    override fun load(): Set<String>? = practised ?: real.load()

    override fun save(stations: Set<String>) {
        practised = stations
    }
}
