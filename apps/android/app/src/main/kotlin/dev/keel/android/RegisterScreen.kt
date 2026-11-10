package dev.keel.android

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.PrimaryScrollableTabRow
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import dev.keel.register.JobKind
import dev.keel.register.JobState
import dev.keel.register.ModifierSheet
import dev.keel.register.OrderState
import dev.keel.register.RegisterModel
import dev.keel.register.RegisterState
import dev.keel.register.Screen
import dev.keel.register.Tender
import dev.keel.register.Ticket

/** The register: whoever signs in, the menu and the order, and today's orders. */
@Composable
fun RegisterScreen(model: RegisterModel) {
    val state by model.state.collectAsStateWithLifecycle()
    Scaffold { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            state.error?.let { error ->
                Text(
                    error,
                    color = MaterialTheme.colorScheme.error,
                    modifier = Modifier.fillMaxWidth().padding(12.dp).testTag("error"),
                )
            }
            when {
                state.loading && state.error == null -> Loading()
                state.screen == Screen.SIGN_IN -> SignIn(state, model)
                state.screen == Screen.ORDERS -> Orders(state, model)
                state.screen == Screen.PRINTER -> PrinterPage(state, model)
                else -> Register(state, model)
            }
        }
        state.sheet?.let { SheetDialog(it, model) }
        state.tender?.let { TenderDialog(it, model) }
    }
}

@Composable
private fun Loading() {
    Column(
        Modifier.fillMaxSize(),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) { CircularProgressIndicator() }
}

@Composable
private fun SignIn(state: RegisterState, model: RegisterModel) {
    Column(Modifier.fillMaxSize().padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        Text("Who's working?", style = MaterialTheme.typography.headlineMedium)
        Spacer(Modifier.height(24.dp))
        FlowRow(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            for (member in state.team) {
                Button(onClick = { model.signIn(member) }, modifier = Modifier.width(160.dp).height(88.dp).testTag("member:${member.name}")) {
                    Text(member.name, style = MaterialTheme.typography.titleMedium)
                }
            }
        }
    }
}

@Composable
private fun Register(state: RegisterState, model: RegisterModel) {
    Row(Modifier.fillMaxSize()) {
        Column(Modifier.weight(2f).fillMaxHeight()) {
            Row(Modifier.fillMaxWidth().padding(horizontal = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(state.member?.name.orEmpty(), Modifier.weight(1f), style = MaterialTheme.typography.titleMedium)
                TextButton(onClick = model::showOrders, modifier = Modifier.testTag("orders")) { Text("Today's orders") }
                TextButton(onClick = model::showPrinter, modifier = Modifier.testTag("printer")) { Text("Printer") }
                TextButton(onClick = model::signOut, modifier = Modifier.testTag("sign-out")) { Text("Sign out") }
            }
            if (state.pages.isNotEmpty()) {
                PrimaryScrollableTabRow(selectedTabIndex = state.page) {
                    state.pages.forEachIndexed { index, page ->
                        Tab(selected = index == state.page, onClick = { model.showPage(index) }, text = { Text(page.name) })
                    }
                }
            }
            LazyVerticalGrid(
                columns = GridCells.Adaptive(minSize = 140.dp),
                modifier = Modifier.fillMaxSize().padding(8.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                items(state.pages.getOrNull(state.page)?.buttons.orEmpty(), key = { it.variant }) { button ->
                    Button(
                        onClick = { model.tap(button) },
                        modifier = Modifier.height(88.dp).testTag("button:${button.variant}"),
                    ) {
                        Column(horizontalAlignment = Alignment.CenterHorizontally) {
                            Text(button.name)
                            Text(
                                button.price + if (button.required) " •" else "",
                                style = MaterialTheme.typography.labelSmall,
                            )
                        }
                    }
                }
            }
        }
        TicketPane(state, model, Modifier.weight(1f))
    }
}

@Composable
private fun TicketPane(state: RegisterState, model: RegisterModel, modifier: Modifier) {
    Column(modifier.fillMaxHeight().padding(16.dp)) {
        val ticket = state.ticket
        Text("Order", style = MaterialTheme.typography.titleLarge)
        LazyColumn(Modifier.weight(1f).fillMaxWidth()) {
            items(ticket?.lines.orEmpty(), key = { it.line }) { line ->
                Column(Modifier.fillMaxWidth().padding(vertical = 4.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text("${line.quantity} × ${line.name}", Modifier.weight(1f))
                        Text(line.amount)
                    }
                    for (words in line.modifiers) Text(words, style = MaterialTheme.typography.bodySmall)
                    Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                        if (line.count != null) {
                            TextButton(onClick = { model.fewer(line) }, modifier = Modifier.testTag("fewer:${line.line}")) { Text("−") }
                            TextButton(onClick = { model.more(line) }, modifier = Modifier.testTag("more:${line.line}")) { Text("+") }
                        }
                        TextButton(onClick = { model.remove(line) }, modifier = Modifier.testTag("remove:${line.line}")) { Text("Remove") }
                    }
                }
            }
        }
        state.change?.let { Text("Change: $it", style = MaterialTheme.typography.titleMedium, modifier = Modifier.testTag("change")) }
        if (ticket != null) {
            HorizontalDivider()
            ticket.tax?.let { Text(it, style = MaterialTheme.typography.bodySmall) }
            Text("Total ${ticket.total}", style = MaterialTheme.typography.titleMedium, modifier = Modifier.testTag("total"))
            Spacer(Modifier.height(8.dp))
            Button(
                onClick = model::openTender,
                enabled = ticket.lines.isNotEmpty(),
                modifier = Modifier.fillMaxWidth().testTag("pay"),
            ) { Text("Cash ${ticket.cashDue}") }
            OutlinedButton(onClick = model::abandon, modifier = Modifier.fillMaxWidth().testTag("abandon")) { Text("Drop order") }
        }
    }
}

@Composable
private fun SheetDialog(sheet: ModifierSheet, model: RegisterModel) {
    AlertDialog(
        onDismissRequest = model::closeSheet,
        title = { Text("${sheet.item.name} ${sheet.item.price}") },
        text = {
            Column(Modifier.heightIn(max = 480.dp).verticalScroll(rememberScrollState())) {
                for (group in sheet.shown) {
                    val bounds = when {
                        group.min == group.max -> "choose ${group.min}"
                        group.min > 0 -> "choose ${group.min} to ${group.max}"
                        else -> "up to ${group.max}"
                    }
                    Text(
                        "${group.name} ($bounds)",
                        style = MaterialTheme.typography.titleSmall,
                        color = if (sheet.satisfied(group)) MaterialTheme.colorScheme.onSurface else MaterialTheme.colorScheme.error,
                        modifier = Modifier.padding(top = 8.dp),
                    )
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        for (option in group.modifiers) {
                            val count = sheet.count(option.modifier)
                            FilterChip(
                                selected = count > 0,
                                onClick = { model.choose(group, option) },
                                label = { Text(option.name + (if (count > 1) " ×$count" else "") + " " + option.price) },
                                modifier = Modifier.testTag("modifier:${option.modifier}"),
                            )
                        }
                    }
                }
                Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(top = 12.dp)) {
                    Text("How many", Modifier.weight(1f))
                    TextButton(onClick = { model.sheetQuantity(sheet.quantity - 1) }) { Text("−") }
                    Text(sheet.quantity.toString(), modifier = Modifier.testTag("sheet:quantity"))
                    TextButton(onClick = { model.sheetQuantity(sheet.quantity + 1) }) { Text("+") }
                }
            }
        },
        confirmButton = {
            Button(onClick = model::addFromSheet, enabled = sheet.complete, modifier = Modifier.testTag("sheet:add")) { Text("Add") }
        },
        dismissButton = { TextButton(onClick = model::closeSheet) { Text("Cancel") } },
    )
}

@Composable
private fun TenderDialog(tender: Tender, model: RegisterModel) {
    AlertDialog(
        onDismissRequest = model::closeTender,
        title = { Text("Cash: ${tender.due.text} due") },
        text = {
            Column {
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    tender.offers.forEachIndexed { index, offer ->
                        Button(onClick = { model.pay(offer) }, modifier = Modifier.testTag("offer:$index")) { Text(offer.text) }
                    }
                }
                Spacer(Modifier.height(16.dp))
                Text(
                    tender.typed?.text ?: "Type an amount",
                    style = MaterialTheme.typography.headlineSmall,
                    modifier = Modifier.testTag("typed"),
                )
                for (row in listOf(listOf(1, 2, 3), listOf(4, 5, 6), listOf(7, 8, 9))) {
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        for (digit in row) Key(digit.toString(), "digit:$digit") { model.typeDigit(digit) }
                    }
                }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Key("⌫", "erase") { model.eraseDigit() }
                    Key("0", "digit:0") { model.typeDigit(0) }
                    Key("00", "digit:00") {
                        model.typeDigit(0)
                        model.typeDigit(0)
                    }
                }
            }
        },
        confirmButton = {
            val typed = tender.typed
            Button(onClick = { typed?.let(model::pay) }, enabled = typed != null, modifier = Modifier.testTag("tender:typed")) {
                Text("Take ${typed?.text.orEmpty()}")
            }
        },
        dismissButton = { TextButton(onClick = model::closeTender) { Text("Cancel") } },
    )
}

@Composable
private fun Key(label: String, tag: String, onClick: () -> Unit) {
    OutlinedButton(onClick = onClick, modifier = Modifier.width(72.dp).testTag(tag)) { Text(label) }
}

@Composable
private fun Orders(state: RegisterState, model: RegisterModel) {
    Column(Modifier.fillMaxSize().padding(16.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text("Today's orders", Modifier.weight(1f), style = MaterialTheme.typography.titleLarge)
            TextButton(onClick = model::showRegister, modifier = Modifier.testTag("back")) { Text("Back") }
        }
        if (state.orders.isEmpty()) Text("None yet today.")
        LazyColumn {
            items(state.orders.reversed(), key = { it.order }) { order ->
                OrderRow(order, onOpen = { model.resume(order) }, onReprint = { model.reprint(order) })
            }
        }
    }
}

@Composable
private fun OrderRow(order: Ticket, onOpen: () -> Unit, onReprint: () -> Unit) {
    val state = when (order.state) {
        OrderState.OPEN -> "Open"
        OrderState.CLOSED -> "Paid"
        OrderState.VOIDED -> "Voided"
        OrderState.ABANDONED -> "Dropped"
    }
    Row(
        Modifier
            .fillMaxWidth()
            .clickable(enabled = order.state == OrderState.OPEN, onClick = onOpen)
            .padding(vertical = 12.dp)
            .testTag("order:${order.order}"),
    ) {
        Text(state, Modifier.width(96.dp))
        Text(order.lines.joinToString { "${it.quantity} × ${it.name}" }.ifEmpty { "Nothing on it" }, Modifier.weight(1f))
        Text(order.total)
        if (order.state == OrderState.CLOSED) {
            TextButton(onClick = onReprint, modifier = Modifier.testTag("reprint:${order.order}")) { Text("Reprint") }
        }
    }
    HorizontalDivider()
}

@Composable
private fun PrinterPage(state: RegisterState, model: RegisterModel) {
    var host by rememberSaveable { mutableStateOf(state.printer?.host.orEmpty()) }
    var port by rememberSaveable { mutableStateOf((state.printer?.port ?: 9100).toString()) }
    Column(Modifier.fillMaxSize().padding(16.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text("Printer", Modifier.weight(1f), style = MaterialTheme.typography.titleLarge)
            TextButton(onClick = model::refreshJobs, modifier = Modifier.testTag("refresh")) { Text("Refresh") }
            TextButton(onClick = model::showRegister, modifier = Modifier.testTag("back")) { Text("Back") }
        }
        Text("Where the receipt printer listens on the store's network.")
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            OutlinedTextField(host, { host = it }, label = { Text("Address") }, singleLine = true, modifier = Modifier.weight(1f).testTag("printer:host"))
            OutlinedTextField(port, { port = it }, label = { Text("Port") }, singleLine = true, modifier = Modifier.width(120.dp).testTag("printer:port"))
            Button(onClick = { model.setPrinter(host, port) }, modifier = Modifier.testTag("printer:save")) { Text("Save") }
        }
        Spacer(Modifier.height(16.dp))
        Text("Jobs", style = MaterialTheme.typography.titleMedium)
        if (state.jobs.isEmpty()) Text("Nothing waiting to print.")
        LazyColumn {
            items(state.jobs, key = { it.job }) { job ->
                Row(Modifier.fillMaxWidth().padding(vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                    val kind = when (job.kind) {
                        JobKind.RECEIPT -> "Receipt"
                        JobKind.DRAWER -> "Cash drawer"
                    }
                    val where = when (job.state) {
                        JobState.QUEUED -> if (job.attempts == 0) "Waiting" else "Waiting, tried ${job.attempts}×"
                        JobState.SENDING -> "Printing"
                        JobState.UNCONFIRMED -> "Did it print?"
                    }
                    Text("$kind: $where", Modifier.weight(1f))
                    if (job.state == JobState.UNCONFIRMED) {
                        TextButton(onClick = { model.resolve(job, printed = true) }) { Text("It printed") }
                        TextButton(onClick = { model.resolve(job, printed = false) }) { Text("Print again") }
                    }
                }
                HorizontalDivider()
            }
        }
    }
}
