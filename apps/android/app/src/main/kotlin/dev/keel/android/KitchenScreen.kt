package dev.keel.android

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.unit.dp
import dev.keel.register.Aging
import dev.keel.register.KdsItem
import dev.keel.register.KdsTicket
import dev.keel.register.ModifierPrefix
import dev.keel.register.RegisterModel
import dev.keel.register.RegisterState

/**
 * The kitchen screen (ADR-0034, decision 6): the tickets of the stations it shows, oldest first,
 * each aged by colour and in words, so colour isn't needed to read it; "No" and "Extra"
 * modifiers, notes and allergens each set apart. A tap selects and bumps; a bump bar's keys,
 * which a keyboard sends, move the selection, bump, and recall the last bumped.
 */
@Composable
fun KitchenPage(state: RegisterState, model: RegisterModel) {
    val kds = state.kds ?: return
    val focus = remember { FocusRequester() }
    LaunchedEffect(Unit) { focus.requestFocus() }
    Column(
        Modifier
            .fillMaxSize()
            .focusRequester(focus)
            .focusable()
            .onPreviewKeyEvent { event ->
                if (event.type != KeyEventType.KeyDown) return@onPreviewKeyEvent false
                // A key held down bumps or recalls once: its repeats do nothing. Tab is left to
                // move between the screen's buttons. Found in review.
                if (event.nativeKeyEvent.repeatCount > 0) return@onPreviewKeyEvent true
                when (event.key) {
                    Key.DirectionRight, Key.DirectionDown -> model.select(1)
                    Key.DirectionLeft, Key.DirectionUp -> model.select(-1)
                    Key.Enter, Key.NumPadEnter, Key.Spacebar -> model.bumpSelected()
                    Key.R, Key.Backspace -> model.recallLast()
                    else -> return@onPreviewKeyEvent false
                }
                true
            }
            .padding(8.dp),
    ) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            val names = state.stations.filter { it.station in kds.stations }.joinToString { it.name }
            Text("Kitchen: $names", Modifier.weight(1f), style = MaterialTheme.typography.titleLarge)
            TextButton(onClick = model::chooseStations, modifier = Modifier.testTag("kitchen:stations")) { Text("Stations") }
            TextButton(onClick = model::recallLast, modifier = Modifier.testTag("kitchen:recall")) { Text("Recall last") }
            TextButton(onClick = model::closeKitchen, modifier = Modifier.testTag("kitchen:exit")) { Text("Exit") }
        }
        if (kds.choosing) {
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                for (station in state.stations) {
                    FilterChip(
                        selected = station.station in kds.stations,
                        onClick = { model.toggleStation(station.station) },
                        label = { Text(if (station.expo) "${station.name} (expo)" else station.name) },
                        modifier = Modifier.testTag("station:${station.name}"),
                    )
                }
            }
        }
        if (kds.stale) {
            Text(
                "Can't read the tickets: their ages are standing still. Trying again.",
                color = MaterialTheme.colorScheme.error,
                modifier = Modifier.testTag("kitchen:stale"),
            )
        }
        // Tickets no screen showed in time, which every device tells its staff (ADR-0034).
        for (alert in state.alerts) {
            Text(alert.words, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.labelLarge)
        }
        // Each station's day so far: how many it bumped, and its average time.
        Text(
            kds.view.times.joinToString("   ") { times ->
                val average = times.averageSeconds?.let { " avg ${clock(it)}" }.orEmpty()
                "${times.name}: ${times.bumped} bumped$average"
            },
            style = MaterialTheme.typography.labelLarge,
            modifier = Modifier.padding(vertical = 4.dp).testTag("kitchen:times"),
        )
        if (kds.view.tickets.isEmpty()) {
            Text("No tickets.", style = MaterialTheme.typography.headlineSmall, modifier = Modifier.padding(24.dp).testTag("kitchen:empty"))
        }
        LazyVerticalGrid(
            columns = GridCells.Adaptive(minSize = 240.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
            modifier = Modifier.fillMaxSize(),
        ) {
            items(kds.view.tickets, key = { it.ticket }) { ticket ->
                TicketCard(
                    ticket,
                    selected = ticket.ticket == kds.selectedTicket?.ticket,
                    showStation = kds.stations.size > 1,
                    bumping = ticket.ticket in kds.bumping,
                    model = model,
                )
            }
        }
    }
}

/** `seconds` as minutes and seconds: "4:07". */
private fun clock(seconds: Long): String = "${seconds / 60}:${(seconds % 60).toString().padStart(2, '0')}"

@Composable
private fun TicketCard(ticket: KdsTicket, selected: Boolean, showStation: Boolean, bumping: Boolean, model: RegisterModel) {
    val scheme = MaterialTheme.colorScheme
    // Aged by colour, and in words: a cook who can't tell the colours apart reads them.
    val (container, aged) = when (ticket.aging) {
        Aging.FRESH -> scheme.surfaceVariant to ""
        Aging.LATE_ISH -> scheme.tertiaryContainer to "  LATE-ISH"
        Aging.LATE -> scheme.errorContainer to "  LATE"
    }
    Card(
        colors = CardDefaults.cardColors(containerColor = container),
        border = if (selected) BorderStroke(4.dp, scheme.primary) else null,
        modifier = Modifier.fillMaxWidth().clickable { model.select(ticket) }.testTag("ticket:${ticket.ticket}"),
    ) {
        Column(Modifier.padding(12.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("#${ticket.code}", Modifier.weight(1f), style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.Bold)
                Text(clock(ticket.ageSeconds) + aged, style = MaterialTheme.typography.titleMedium, modifier = Modifier.testTag("age:${ticket.ticket}"))
            }
            Text(
                ticket.mode.uppercase() + if (showStation) " · ${ticket.stationName}" else "",
                style = MaterialTheme.typography.labelLarge,
            )
            if (ticket.rerouted) {
                Text("Printed: no screen showed it in time", color = scheme.error, style = MaterialTheme.typography.labelMedium)
            }
            for (item in ticket.items) ItemRows(item)
            Button(
                onClick = { model.bump(ticket) },
                enabled = !bumping,
                modifier = Modifier.fillMaxWidth().padding(top = 8.dp).testTag("bump:${ticket.ticket}"),
            ) { Text("Bump") }
        }
    }
}

@Composable
private fun ItemRows(item: KdsItem) {
    val scheme = MaterialTheme.colorScheme
    val struck = if (item.void) TextDecoration.LineThrough else TextDecoration.None
    Column(Modifier.padding(top = 6.dp).testTag("item:${item.line}:${item.name}")) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                "${item.quantity} ${item.name}",
                Modifier.weight(1f),
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.Bold,
                textDecoration = struck,
            )
            if (item.void) Text("VOID", color = scheme.error, fontWeight = FontWeight.Bold, modifier = Modifier.testTag("voided:${item.line}"))
            item.ready?.let { made -> Text(if (made) "✓ made" else "waiting", style = MaterialTheme.typography.labelMedium) }
        }
        item.componentOf?.let { Text("for $it", style = MaterialTheme.typography.labelMedium) }
        for (modifier in item.modifiers) {
            // "No" and "Extra" read in words and in weight, as well as in colour.
            val (prefix, colour) = when (modifier.prefix) {
                ModifierPrefix.NO -> "NO " to scheme.error
                ModifierPrefix.EXTRA -> "EXTRA " to scheme.primary
                ModifierPrefix.LIGHT -> "LIGHT " to scheme.secondary
                ModifierPrefix.SIDE -> "SIDE " to scheme.onSurface
                ModifierPrefix.SUB -> "SUB " to scheme.onSurface
                ModifierPrefix.PLAIN -> "" to scheme.onSurface
            }
            val count = if (modifier.quantity > 1) "${modifier.quantity} x " else ""
            Text(
                prefix + count + modifier.name,
                color = colour,
                fontWeight = if (modifier.prefix == ModifierPrefix.PLAIN) FontWeight.Normal else FontWeight.Bold,
                textDecoration = struck,
                modifier = Modifier.padding(start = (12 * modifier.depth).dp),
            )
        }
        item.notes?.let { notes ->
            Text(
                "NOTE: $notes",
                fontWeight = FontWeight.Bold,
                modifier = Modifier.padding(top = 2.dp).background(scheme.tertiaryContainer).padding(horizontal = 4.dp),
            )
        }
        if (item.allergens.isNotEmpty()) {
            Text(
                "⚠ ALLERGENS: " + item.allergens.joinToString(),
                fontWeight = FontWeight.Bold,
                color = scheme.onErrorContainer,
                modifier = Modifier.padding(top = 2.dp).background(scheme.errorContainer).padding(horizontal = 4.dp).testTag("allergens:${item.line}"),
            )
        }
    }
}
