package dev.keel.android

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import dev.keel.register.RegisterModel
import dev.keel.register.RegisterState
import dev.keel.register.STOCK_REASONS
import dev.keel.register.StockItem
import dev.keel.register.StockMovement
import dev.keel.register.StockSheet
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter

/**
 * The stock screen (ADR-0036): the items tracked, those below zero first, each with what is on
 * hand, set aside and available; the item chosen, what moved it since its latest count, and a
 * movement or a count for it. An item not tracked is chosen from the menu, and tracked from its
 * first movement or count.
 */
@Composable
fun StockPage(state: RegisterState, model: RegisterModel) {
    val sheet = state.stock ?: return
    Row(Modifier.fillMaxSize().padding(16.dp), horizontalArrangement = Arrangement.spacedBy(16.dp)) {
        Column(Modifier.weight(1f)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(t("Stock"), Modifier.weight(1f), style = MaterialTheme.typography.titleLarge)
                TextButton(onClick = model::chooseStockItem, modifier = Modifier.testTag("stock:track")) { Text(t("Track an item")) }
                TextButton(onClick = model::closeStock, modifier = Modifier.testTag("back")) { Text(t("Back")) }
            }
            // The error is shown above every screen already: found in review, it showed twice.
            if (sheet.choosing) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(t("Choose an item to track"), Modifier.weight(1f), style = MaterialTheme.typography.titleSmall)
                    TextButton(onClick = model::stopChoosingStock, modifier = Modifier.testTag("stock:stop-choosing")) { Text(t("Done")) }
                }
                // At most half the height, scrolled: found in review, a large menu hid the list.
                FlowRow(
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    modifier = Modifier.heightIn(max = 240.dp).verticalScroll(rememberScrollState()),
                ) {
                    for (button in state.pages.flatMap { it.buttons }.distinctBy { it.variant }) {
                        OutlinedButton(onClick = { model.trackStock(button) }, modifier = Modifier.testTag("stock-track:${button.variant}")) {
                            Text(button.name)
                        }
                    }
                }
            }
            if (sheet.items.isEmpty()) Text(t("No item's stock is tracked yet."), modifier = Modifier.padding(vertical = 8.dp))
            LazyColumn(Modifier.weight(1f)) {
                items(sheet.items, key = { it.variant }) { item -> StockRow(item, item.variant == sheet.selected?.variant, model) }
            }
        }
        // Scrolled: found in review, a day's sales since the last count ran off the screen.
        sheet.selected?.let { item ->
            Column(Modifier.weight(1f).verticalScroll(rememberScrollState())) { StockDetail(item, sheet, model) }
        }
    }
}

@Composable
private fun StockRow(item: StockItem, selected: Boolean, model: RegisterModel) {
    val colour = if (item.onHand.negative) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurface
    Row(
        Modifier.fillMaxWidth().clickable { model.selectStock(item) }.padding(vertical = 8.dp).testTag("stock-item:${item.variant}"),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(item.name + if (selected) "  ◀" else "", Modifier.weight(1f), style = MaterialTheme.typography.titleMedium)
        Text(item.onHand.text, color = colour, style = MaterialTheme.typography.titleMedium, modifier = Modifier.testTag("on-hand:${item.variant}"))
        if (item.reserved.micros != 0L) Text("  (${item.available.text} free)", style = MaterialTheme.typography.bodySmall)
        if (item.conflicts > 0) Text("  ⚠", color = MaterialTheme.colorScheme.error)
    }
    HorizontalDivider()
}

@Composable
private fun StockDetail(item: StockItem, sheet: StockSheet, model: RegisterModel) {
    Text(item.name, style = MaterialTheme.typography.titleLarge)
    Text("On hand ${item.onHand.text} · set aside ${item.reserved.text} · available ${item.available.text}")
    item.countedAt?.let { Text("Last counted ${timeOf(it)}", style = MaterialTheme.typography.bodySmall) }
    if (item.onHand.negative) Text(t("More sold than was there: count it."), color = MaterialTheme.colorScheme.error)
    if (item.conflicts > 0) {
        val records = if (item.conflicts == 1L) "1 record" else "${item.conflicts} records"
        Text("$records of its stock didn't count, as in another unit: look at its history.", color = MaterialTheme.colorScheme.error)
    }
    val entry = sheet.entry
    if (entry == null) {
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.padding(vertical = 8.dp)) {
            for ((movement, words) in listOf(
                StockMovement.RECEIVED to "Receive",
                StockMovement.WASTED to "Waste",
                StockMovement.ADJUSTED to "Adjust",
                StockMovement.RESERVED to "Set aside",
                StockMovement.RELEASED to "Release",
            )) {
                OutlinedButton(onClick = { model.startStockEntry(movement) }, modifier = Modifier.testTag("stock-move:${movement.name}")) { Text(words) }
            }
            OutlinedButton(onClick = { model.startStockEntry(null) }, modifier = Modifier.testTag("stock-count")) { Text(t("Count")) }
        }
    } else {
        val what = when (entry.movement) {
            StockMovement.RECEIVED -> "Received"
            StockMovement.WASTED -> "Wasted"
            StockMovement.ADJUSTED -> if (entry.down) "Take off" else "Add"
            StockMovement.RESERVED -> "Set aside"
            StockMovement.RELEASED -> "Release"
            null -> "Counted: on hand now"
        }
        val unit = if (item.onHand.unit == "each") "" else " (thousandths of ${item.onHand.unit})"
        Text("$what: ${entry.typed}$unit", style = MaterialTheme.typography.headlineSmall, modifier = Modifier.testTag("stock:typed"))
        if (entry.movement == StockMovement.ADJUSTED) {
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                FilterChip(selected = !entry.down, onClick = { model.stockDown(false) }, label = { Text(t("Add")) })
                FilterChip(selected = entry.down, onClick = { model.stockDown(true) }, label = { Text(t("Take off")) })
            }
        }
        if (entry.movement != null) {
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                for (reason in STOCK_REASONS) {
                    FilterChip(selected = entry.reason == reason.code, onClick = { model.stockReason(reason.code) }, label = { Text(t(reason.words)) })
                }
            }
        }
        // Keys that share the pane's width, and Record on a row of its own: found in review, a
        // narrow pane squeezed it.
        Column(Modifier.widthIn(max = 280.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            for (row in listOf(listOf("1", "2", "3"), listOf("4", "5", "6"), listOf("7", "8", "9"), listOf("⌫", "0", ""))) {
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    for (key in row) {
                        val keyModifier = Modifier.weight(1f)
                        when (key) {
                            "" -> Text("", keyModifier)
                            "⌫" -> OutlinedButton(onClick = model::eraseStockDigit, modifier = keyModifier.testTag("stock-erase")) { Text(key) }
                            else -> OutlinedButton(onClick = { model.typeStockDigit(key.toInt()) }, modifier = keyModifier.testTag("stock-digit:$key")) { Text(key) }
                        }
                    }
                }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                TextButton(onClick = model::cancelStockEntry, modifier = Modifier.weight(1f)) { Text(t("Cancel")) }
                Button(onClick = model::confirmStockEntry, enabled = !sheet.working, modifier = Modifier.weight(1f).testTag("stock:confirm")) { Text(t("Record")) }
            }
        }
    }
    Text(t("Since the last count"), style = MaterialTheme.typography.titleSmall, modifier = Modifier.padding(top = 12.dp))
    if (sheet.history.isEmpty()) Text(t("Nothing yet."), style = MaterialTheme.typography.bodySmall)
    for (line in sheet.history) {
        Row(Modifier.fillMaxWidth()) {
            Text(timeOf(line.at), Modifier.padding(end = 8.dp), style = MaterialTheme.typography.bodySmall)
            Text(line.what, Modifier.weight(1f))
            Text(line.quantity)
        }
    }
}

private val shortTime = DateTimeFormatter.ofPattern("d MMM HH:mm")

/** [micros] since the epoch, as the device's clock reads it. */
private fun timeOf(micros: Long): String =
    shortTime.format(Instant.ofEpochMilli(micros / 1_000).atZone(ZoneId.systemDefault()))
