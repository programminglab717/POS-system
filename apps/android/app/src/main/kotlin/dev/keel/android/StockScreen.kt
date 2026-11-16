package dev.keel.android

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
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
                Text("Stock", Modifier.weight(1f), style = MaterialTheme.typography.titleLarge)
                TextButton(onClick = model::chooseStockItem, modifier = Modifier.testTag("stock:track")) { Text("Track an item") }
                TextButton(onClick = model::closeStock, modifier = Modifier.testTag("back")) { Text("Back") }
            }
            state.error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            if (sheet.choosing) {
                Text("Choose an item to track", style = MaterialTheme.typography.titleSmall)
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    for (button in state.pages.flatMap { it.buttons }) {
                        OutlinedButton(onClick = { model.trackStock(button) }, modifier = Modifier.testTag("stock-track:${button.variant}")) {
                            Text(button.name)
                        }
                    }
                }
            }
            if (sheet.items.isEmpty()) Text("No item's stock is tracked yet.", modifier = Modifier.padding(vertical = 8.dp))
            LazyColumn {
                items(sheet.items, key = { it.variant }) { item -> StockRow(item, item.variant == sheet.selected?.variant, model) }
            }
        }
        sheet.selected?.let { item -> Column(Modifier.weight(1f)) { StockDetail(item, sheet, model) } }
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
    }
    HorizontalDivider()
}

@Composable
private fun StockDetail(item: StockItem, sheet: StockSheet, model: RegisterModel) {
    Text(item.name, style = MaterialTheme.typography.titleLarge)
    Text("On hand ${item.onHand.text} · set aside ${item.reserved.text} · available ${item.available.text}")
    if (item.onHand.negative) Text("More sold than was there: count it.", color = MaterialTheme.colorScheme.error)
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
            OutlinedButton(onClick = { model.startStockEntry(null) }, modifier = Modifier.testTag("stock-count")) { Text("Count") }
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
                FilterChip(selected = !entry.down, onClick = { model.stockDown(false) }, label = { Text("Add") })
                FilterChip(selected = entry.down, onClick = { model.stockDown(true) }, label = { Text("Take off") })
            }
        }
        if (entry.movement != null) {
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                for (reason in STOCK_REASONS) {
                    FilterChip(selected = entry.reason == reason.code, onClick = { model.stockReason(reason.code) }, label = { Text(reason.words) })
                }
            }
        }
        for (row in listOf(listOf(1, 2, 3), listOf(4, 5, 6), listOf(7, 8, 9))) {
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                for (digit in row) {
                    OutlinedButton(onClick = { model.typeStockDigit(digit) }, modifier = Modifier.width(72.dp).testTag("stock-digit:$digit")) { Text("$digit") }
                }
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedButton(onClick = model::eraseStockDigit, modifier = Modifier.width(72.dp)) { Text("⌫") }
            OutlinedButton(onClick = { model.typeStockDigit(0) }, modifier = Modifier.width(72.dp).testTag("stock-digit:0")) { Text("0") }
            TextButton(onClick = model::cancelStockEntry) { Text("Cancel") }
            Button(onClick = model::confirmStockEntry, enabled = !sheet.working, modifier = Modifier.testTag("stock:confirm")) { Text("Record") }
        }
    }
    Text("Since the last count", style = MaterialTheme.typography.titleSmall, modifier = Modifier.padding(top = 12.dp))
    for (line in sheet.history) {
        Row(Modifier.fillMaxWidth()) {
            Text(line.what, Modifier.weight(1f))
            Text(line.quantity)
        }
    }
}
