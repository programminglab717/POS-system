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
import androidx.compose.foundation.lazy.grid.itemsIndexed
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
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import dev.keel.register.Approval
import dev.keel.register.CASH_REASONS
import dev.keel.register.CashEntry
import dev.keel.register.CashPurpose
import dev.keel.register.Drawer
import dev.keel.register.DISCOUNT_REASONS
import dev.keel.register.DiscountSheet
import dev.keel.register.NotesSheet
import dev.keel.register.PRICE_REASONS
import dev.keel.register.PriceSheet
import dev.keel.register.TicketDiscount
import dev.keel.register.percentWords
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
            // A PIN pad shows its own refusal.
            state.error?.takeIf { state.pinFor == null && state.approval?.approver == null }?.let { error ->
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
                state.screen == Screen.DRAWER -> DrawerPage(state, model)
                else -> Register(state, model)
            }
        }
        state.sheet?.let { SheetDialog(it, model) }
        state.tender?.let { TenderDialog(it, model) }
        state.discount?.let { DiscountDialog(it, state.error, model) }
        state.price?.let { PriceDialog(it, state.error, model) }
        state.cash?.let { CashDialog(it, state.error, model) }
        state.notes?.let { NotesDialog(it, model) }
        state.pinFor?.let { member ->
            PinDialog("${member.name}'s PIN", state.error, onEnter = model::enterPin, onCancel = model::cancelPin)
        }
        state.approval?.let { ApprovalDialog(it, state.error, model) }
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
                state.sync?.let { sync ->
                    // Settled first: until it is, the register refuses every sale, whatever it hears.
                    // Found in review: "Synced" showed while sales were refused.
                    val words = when {
                        !sync.settled -> "Joining…"
                        sync.serving -> "Store hub"
                        sync.hubReachable -> "Synced"
                        else -> "Offline"
                    }
                    Text(words, style = MaterialTheme.typography.labelLarge, modifier = Modifier.testTag("sync"))
                }
                TextButton(onClick = model::showOrders, modifier = Modifier.testTag("orders")) { Text("Today's orders") }
                TextButton(onClick = model::showDrawer, modifier = Modifier.testTag("drawer")) { Text("Drawer") }
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
                // Keyed by place: a page may hold a variant twice. Found in review: keyed by the
                // variant, such a page crashed the register.
                itemsIndexed(state.pages.getOrNull(state.page)?.buttons.orEmpty(), key = { index, _ -> index }) { _, button ->
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
        if (state.askDrawer) OpenDrawerBanner(model)
        Text("Order", style = MaterialTheme.typography.titleLarge)
        LazyColumn(Modifier.weight(1f).fillMaxWidth()) {
            items(ticket?.lines.orEmpty(), key = { it.line }) { line ->
                Column(Modifier.fillMaxWidth().padding(vertical = 4.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text("${line.quantity} × ${line.name}", Modifier.weight(1f))
                        Text(line.amount)
                    }
                    for (words in line.modifiers) Text(words, style = MaterialTheme.typography.bodySmall)
                    line.repriced?.let { Text(it, style = MaterialTheme.typography.bodySmall, modifier = Modifier.testTag("repriced:${line.line}")) }
                    line.notes?.let { Text("“$it”", style = MaterialTheme.typography.bodySmall, modifier = Modifier.testTag("notes:${line.line}")) }
                    for (discount in line.discounts) DiscountRow(discount, model)
                    // Wrapped, so every button stays reachable on a narrow pane. Found in review:
                    // the later ones were squeezed to nothing on a phone.
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                        if (line.count != null) {
                            TextButton(onClick = { model.fewer(line) }, modifier = Modifier.testTag("fewer:${line.line}")) { Text("−") }
                            TextButton(onClick = { model.more(line) }, modifier = Modifier.testTag("more:${line.line}")) { Text("+") }
                        }
                        TextButton(onClick = { model.remove(line) }, modifier = Modifier.testTag("remove:${line.line}")) { Text("Remove") }
                        TextButton(onClick = { model.openDiscount(line) }, modifier = Modifier.testTag("discount:${line.line}")) { Text("Discount") }
                        TextButton(onClick = { model.openPrice(line) }, modifier = Modifier.testTag("price:${line.line}")) { Text("Price") }
                        TextButton(onClick = { model.openNotes(line) }, modifier = Modifier.testTag("note:${line.line}")) { Text("Note") }
                    }
                    // A line moves to a check of its own, or to another open check of a split order.
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                        for (check in ticket?.checks.orEmpty().filter { it.open && it.lines.none { on -> on.line == line.line } }) {
                            TextButton(onClick = { model.moveLine(line, check.check) }, modifier = Modifier.testTag("move:${check.number}:${line.line}")) {
                                Text("To check ${check.number}")
                            }
                        }
                        TextButton(onClick = { model.moveLine(line, null) }, modifier = Modifier.testTag("move:new:${line.line}")) {
                            Text("Own check")
                        }
                    }
                }
            }
        }
        state.change?.let { Text("Change: $it", style = MaterialTheme.typography.titleMedium, modifier = Modifier.testTag("change")) }
        if (ticket != null) {
            HorizontalDivider()
            for (discount in ticket.discounts) DiscountRow(discount, model)
            ticket.tax?.let { Text(it, style = MaterialTheme.typography.bodySmall) }
            Text("Total ${ticket.total}", style = MaterialTheme.typography.titleMedium, modifier = Modifier.testTag("total"))
            // A split order is paid check by check.
            for (check in ticket.checks) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        "Check ${check.number}: ${check.total}" + if (check.open) "" else " (paid)",
                        Modifier.weight(1f).testTag("check:${check.number}"),
                    )
                    if (check.open) {
                        TextButton(onClick = { model.openTender(check) }, modifier = Modifier.testTag("pay:check:${check.number}")) {
                            Text("Pay ${check.cashDue}")
                        }
                    }
                }
            }
            Spacer(Modifier.height(8.dp))
            if (ticket.checks.isEmpty()) {
                Button(
                    onClick = { model.openTender() },
                    enabled = ticket.lines.isNotEmpty(),
                    modifier = Modifier.fillMaxWidth().testTag("pay"),
                ) { Text("Cash ${ticket.cashDue}") }
            }
            // A paid check holds the split, and the whole order's discounts: neither is offered once
            // one is. Found in review.
            val anyPaid = ticket.checks.any { !it.open }
            FlowRow(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                for (ways in 2..4) {
                    OutlinedButton(
                        onClick = { model.split(ways) },
                        enabled = ticket.lines.isNotEmpty() && !anyPaid,
                        modifier = Modifier.testTag("split:$ways"),
                    ) { Text("Split $ways") }
                }
            }
            OutlinedButton(
                onClick = { model.openDiscount() },
                enabled = ticket.lines.isNotEmpty() && !anyPaid,
                modifier = Modifier.fillMaxWidth().testTag("discount:order"),
            ) { Text("Discount the order") }
            OutlinedButton(onClick = model::abandon, modifier = Modifier.fillMaxWidth().testTag("abandon")) { Text("Drop order") }
        }
    }
}

/**
 * Once someone signs in with no drawer open, a banner asks to open one, with its float: never a
 * dialog in the way, for a sale never waits for a drawer (ADR-0031).
 */
@Composable
private fun OpenDrawerBanner(model: RegisterModel) {
    Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth().testTag("drawer:ask")) {
        Text("No drawer is open.", Modifier.weight(1f), style = MaterialTheme.typography.bodySmall)
        TextButton(onClick = { model.beginCash(CashPurpose.FLOAT) }, modifier = Modifier.testTag("drawer:open")) { Text("Open it") }
        TextButton(onClick = model::dismissDrawer, modifier = Modifier.testTag("drawer:later")) { Text("Not now") }
    }
}

/** The keypad for an amount on the drawer: a float, a movement with its reason, or the blind count. */
@Composable
private fun CashDialog(entry: CashEntry, error: String?, model: RegisterModel) {
    val title = when (entry.purpose) {
        CashPurpose.FLOAT -> "Float"
        CashPurpose.PAID_IN -> "Pay in"
        CashPurpose.PAID_OUT -> "Pay out"
        CashPurpose.DROP -> "Drop to the safe"
        CashPurpose.COUNT -> "Count the drawer"
    }
    val moving = entry.purpose in setOf(CashPurpose.PAID_IN, CashPurpose.PAID_OUT, CashPurpose.DROP)
    AlertDialog(
        onDismissRequest = model::cancelCash,
        title = { Text(title) },
        text = {
            Column(Modifier.heightIn(max = 520.dp).verticalScroll(rememberScrollState())) {
                if (moving) {
                    Text("Why", style = MaterialTheme.typography.titleSmall)
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        for (reason in CASH_REASONS) {
                            FilterChip(
                                selected = entry.reason == reason.code,
                                onClick = { model.cashReason(reason.code) },
                                label = { Text(reason.words) },
                                modifier = Modifier.testTag("cash-reason:${reason.code}"),
                            )
                        }
                    }
                }
                if (entry.purpose == CashPurpose.COUNT) {
                    // Blind: what the drawer should hold isn't shown until the count is in.
                    Text("Count what is in the drawer, and type it.", style = MaterialTheme.typography.bodySmall)
                }
                Spacer(Modifier.height(8.dp))
                Text(entry.typed?.text ?: "Type an amount", style = MaterialTheme.typography.headlineSmall, modifier = Modifier.testTag("cash:typed"))
                for (row in listOf(listOf(1, 2, 3), listOf(4, 5, 6), listOf(7, 8, 9))) {
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        for (digit in row) Key(digit.toString(), "cash:digit:$digit") { model.typeCashDigit(digit) }
                    }
                }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Key("⌫", "cash:erase") { model.eraseCashDigit() }
                    Key("0", "cash:digit:0") { model.typeCashDigit(0) }
                    Key("00", "cash:digit:00") {
                        model.typeCashDigit(0)
                        model.typeCashDigit(0)
                    }
                }
                error?.let { Text(it, color = MaterialTheme.colorScheme.error, modifier = Modifier.testTag("cash:error")) }
            }
        },
        confirmButton = {
            Button(onClick = model::confirmCash, modifier = Modifier.testTag("cash:confirm")) {
                Text(if (entry.purpose == CashPurpose.COUNT) "Close the drawer" else "Done")
            }
        },
        dismissButton = { TextButton(onClick = model::cancelCash, modifier = Modifier.testTag("cash:cancel")) { Text("Cancel") } },
    )
}

/** The drawer: what moved through it, its movements, its close, and the day's reports. */
@Composable
private fun DrawerPage(state: RegisterState, model: RegisterModel) {
    Column(Modifier.fillMaxSize().padding(16.dp).verticalScroll(rememberScrollState())) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text("Drawer", Modifier.weight(1f), style = MaterialTheme.typography.titleLarge)
            TextButton(onClick = model::showRegister, modifier = Modifier.testTag("back")) { Text("Back") }
        }
        state.error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        val drawer = state.drawer
        if (drawer == null) {
            Text("No drawer is open.", modifier = Modifier.testTag("drawer:none"))
            Button(onClick = { model.beginCash(CashPurpose.FLOAT) }, modifier = Modifier.testTag("drawer:open")) { Text("Open the drawer") }
        } else {
            DrawerFigures(drawer)
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedButton(onClick = { model.beginCash(CashPurpose.PAID_IN) }, modifier = Modifier.testTag("drawer:in")) { Text("Pay in") }
                OutlinedButton(onClick = { model.beginCash(CashPurpose.PAID_OUT) }, modifier = Modifier.testTag("drawer:out")) { Text("Pay out") }
                OutlinedButton(onClick = { model.beginCash(CashPurpose.DROP) }, modifier = Modifier.testTag("drawer:drop")) { Text("Drop") }
                OutlinedButton(onClick = { model.noSale("change_for_customer") }, modifier = Modifier.testTag("drawer:no-sale")) { Text("No sale") }
                Button(onClick = { model.beginCash(CashPurpose.COUNT) }, modifier = Modifier.testTag("drawer:close")) { Text("Close the drawer") }
            }
        }
        state.closedDrawer?.let { closed ->
            HorizontalDivider(Modifier.padding(vertical = 8.dp))
            Text("Last close", style = MaterialTheme.typography.titleMedium)
            DrawerFigures(closed)
        }
        HorizontalDivider(Modifier.padding(vertical = 8.dp))
        Text("Today's reports", style = MaterialTheme.typography.titleMedium)
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedButton(onClick = { model.showReport(z = false) }, modifier = Modifier.testTag("report:x")) { Text("X report") }
            OutlinedButton(onClick = { model.showReport(z = true) }, modifier = Modifier.testTag("report:z")) { Text("Z report") }
        }
        state.report?.let { report ->
            Text(if (report.z) "Z report, ${report.date}" else "X report, ${report.date}", style = MaterialTheme.typography.titleSmall)
            ReportRow("Orders", report.orders.toString())
            ReportRow("Gross", report.gross)
            ReportRow("Discounts", report.discounts)
            ReportRow("Net", report.net)
            ReportRow("Tax", report.tax)
            ReportRow("Total", report.total, Modifier.testTag("report:total"))
            for ((tender, amount) in report.tenders) ReportRow(tender, amount)
            ReportRow("Voided orders", report.voided.toString())
            report.withoutDrawer?.let { (words, amount) -> ReportRow(words, amount) }
            Button(onClick = { model.printReport(report.z) }, modifier = Modifier.testTag("report:print")) { Text("Print") }
        }
    }
}

@Composable
private fun DrawerFigures(drawer: Drawer) {
    ReportRow("Float", drawer.float)
    ReportRow("Cash sales", drawer.taken, Modifier.testTag(if (drawer.open) "drawer:taken" else "closed:taken"))
    ReportRow("Paid in", drawer.paidIn)
    ReportRow("Paid out", drawer.paidOut)
    ReportRow("Dropped", drawer.dropped)
    ReportRow("No sales", drawer.noSales.toString())
    drawer.expected?.let { ReportRow("Expected", it) }
    drawer.counted?.let { ReportRow("Counted", it) }
    drawer.variance?.let { ReportRow("Over/short", it, Modifier.testTag("closed:variance")) }
}

@Composable
private fun ReportRow(left: String, right: String, modifier: Modifier = Modifier) {
    Row(Modifier.fillMaxWidth()) {
        Text(left, Modifier.weight(1f))
        Text(right, modifier)
    }
}

/** The price sheet: a reason, and a new unit price typed. */
@Composable
private fun PriceDialog(sheet: PriceSheet, error: String?, model: RegisterModel) {
    AlertDialog(
        onDismissRequest = model::closePrice,
        title = { Text("Price: ${sheet.line.name}") },
        text = {
            Column(Modifier.heightIn(max = 520.dp).verticalScroll(rememberScrollState())) {
                Text("Why", style = MaterialTheme.typography.titleSmall)
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    for (reason in PRICE_REASONS) {
                        FilterChip(
                            selected = sheet.reason == reason.code,
                            onClick = { model.priceReason(reason.code) },
                            label = { Text(reason.words) },
                            modifier = Modifier.testTag("price-reason:${reason.code}"),
                        )
                    }
                }
                Spacer(Modifier.height(16.dp))
                Text(
                    sheet.typed?.text ?: "Type the price of one",
                    style = MaterialTheme.typography.headlineSmall,
                    modifier = Modifier.testTag("price:typed"),
                )
                for (row in listOf(listOf(1, 2, 3), listOf(4, 5, 6), listOf(7, 8, 9))) {
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        for (digit in row) Key(digit.toString(), "price:digit:$digit") { model.typePriceDigit(digit) }
                    }
                }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Key("⌫", "price:erase") { model.erasePriceDigit() }
                    Key("0", "price:digit:0") { model.typePriceDigit(0) }
                }
                error?.let { Text(it, color = MaterialTheme.colorScheme.error, modifier = Modifier.testTag("price:error")) }
            }
        },
        confirmButton = {
            Button(onClick = model::setPrice, modifier = Modifier.testTag("price:set")) {
                Text(sheet.typed?.let { "Sell at ${it.text}" } ?: "Sell free")
            }
        },
        dismissButton = { TextButton(onClick = model::closePrice, modifier = Modifier.testTag("price:cancel")) { Text("Cancel") } },
    )
}

/** The notes sheet: the line's notes, for the kitchen and the receipt. */
@Composable
private fun NotesDialog(sheet: NotesSheet, model: RegisterModel) {
    var text by rememberSaveable(sheet.line.line) { mutableStateOf(sheet.line.notes.orEmpty()) }
    AlertDialog(
        onDismissRequest = model::closeNotes,
        title = { Text("Note: ${sheet.line.name}") },
        text = {
            OutlinedTextField(
                value = text,
                onValueChange = { if (it.length <= MAX_NOTE) text = it },
                label = { Text("For the kitchen and the receipt") },
                modifier = Modifier.fillMaxWidth().testTag("notes:text"),
            )
        },
        confirmButton = {
            Button(onClick = { model.saveNotes(text) }, modifier = Modifier.testTag("notes:save")) { Text("Keep") }
        },
        dismissButton = { TextButton(onClick = model::closeNotes, modifier = Modifier.testTag("notes:cancel")) { Text("Cancel") } },
    )
}

/** The most characters a note holds, as the kernel keeps them. */
private const val MAX_NOTE = 500

/** A discount on the ticket: its words, what it took, and a way to take it off. */
@Composable
private fun DiscountRow(discount: TicketDiscount, model: RegisterModel) {
    Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.testTag("discounted:${discount.discount}")) {
        Text(discount.words, Modifier.weight(1f), style = MaterialTheme.typography.bodySmall)
        Text(discount.amount, style = MaterialTheme.typography.bodySmall)
        TextButton(onClick = { model.removeDiscount(discount) }, modifier = Modifier.testTag("undiscount:${discount.discount}")) {
            Text("✕")
        }
    }
}

/** The discount sheet: a reason, then a percentage, or for a line an amount typed. */
@Composable
private fun DiscountDialog(sheet: DiscountSheet, error: String?, model: RegisterModel) {
    AlertDialog(
        onDismissRequest = model::closeDiscount,
        title = { Text(sheet.line?.let { "Discount: ${it.name}" } ?: "Discount the order") },
        text = {
            Column(Modifier.heightIn(max = 520.dp).verticalScroll(rememberScrollState())) {
                Text("Why", style = MaterialTheme.typography.titleSmall)
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    for (reason in DISCOUNT_REASONS) {
                        FilterChip(
                            selected = sheet.reason == reason.code,
                            onClick = { model.discountReason(reason.code) },
                            label = { Text(reason.words) },
                            modifier = Modifier.testTag("reason:${reason.code}"),
                        )
                    }
                }
                Spacer(Modifier.height(8.dp))
                Text("How much", style = MaterialTheme.typography.titleSmall)
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    for (percent in sheet.percents) {
                        Button(onClick = { model.discountPercent(percent) }, modifier = Modifier.testTag("percent:$percent")) {
                            Text(percentWords(percent))
                        }
                    }
                }
                if (sheet.line != null) {
                    Spacer(Modifier.height(16.dp))
                    Text(
                        sheet.typed?.text ?: "Or type an amount off",
                        style = MaterialTheme.typography.headlineSmall,
                        modifier = Modifier.testTag("discount:typed"),
                    )
                    for (row in listOf(listOf(1, 2, 3), listOf(4, 5, 6), listOf(7, 8, 9))) {
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            for (digit in row) Key(digit.toString(), "discount:digit:$digit") { model.typeDiscountDigit(digit) }
                        }
                    }
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        Key("⌫", "discount:erase") { model.eraseDiscountDigit() }
                        Key("0", "discount:digit:0") { model.typeDiscountDigit(0) }
                    }
                }
                error?.let { Text(it, color = MaterialTheme.colorScheme.error, modifier = Modifier.testTag("discount:error")) }
            }
        },
        confirmButton = {
            if (sheet.line != null) {
                val typed = sheet.typed
                Button(onClick = model::discountAmount, enabled = typed != null, modifier = Modifier.testTag("discount:amount")) {
                    Text("Take ${typed?.text.orEmpty()} off")
                }
            }
        },
        dismissButton = { TextButton(onClick = model::closeDiscount, modifier = Modifier.testTag("discount:cancel")) { Text("Cancel") } },
    )
}

@Composable
private fun SheetDialog(sheet: ModifierSheet, model: RegisterModel) {
    AlertDialog(
        onDismissRequest = model::closeSheet,
        title = { Text("${sheet.item.name} ${sheet.item.price}") },
        text = {
            Column(Modifier.heightIn(max = 480.dp).verticalScroll(rememberScrollState())) {
                for ((index, group) in sheet.shown) {
                    val bounds = when {
                        group.min == group.max -> "choose ${group.min}"
                        group.min > 0 -> "choose ${group.min} to ${group.max}"
                        else -> "up to ${group.max}"
                    }
                    Text(
                        "${group.name} ($bounds)",
                        style = MaterialTheme.typography.titleSmall,
                        color = if (sheet.satisfied(index)) MaterialTheme.colorScheme.onSurface else MaterialTheme.colorScheme.error,
                        modifier = Modifier.padding(top = 8.dp),
                    )
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        for (option in group.modifiers) {
                            val count = sheet.count(index, option.modifier)
                            FilterChip(
                                selected = count > 0,
                                onClick = { model.choose(index, option.modifier) },
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
private fun PinDialog(title: String, error: String?, onEnter: (String) -> Unit, onCancel: () -> Unit) {
    // The digits typed stay on this screen alone: a PIN is never in the register's state, nor in
    // the state Android saves for the activity. Found in review: it was.
    var typed by remember { mutableStateOf("") }
    AlertDialog(
        onDismissRequest = onCancel,
        title = { Text(title) },
        text = {
            Column {
                Text(
                    "●".repeat(typed.length).ifEmpty { "Type the PIN" },
                    style = MaterialTheme.typography.headlineSmall,
                    modifier = Modifier.testTag("pin:typed"),
                )
                error?.let { Text(it, color = MaterialTheme.colorScheme.error, modifier = Modifier.testTag("pin:error")) }
                Spacer(Modifier.height(16.dp))
                for (row in listOf(listOf(1, 2, 3), listOf(4, 5, 6), listOf(7, 8, 9))) {
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        for (digit in row) Key(digit.toString(), "pin:$digit") { if (typed.length < 8) typed += digit }
                    }
                }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Key("⌫", "pin:erase") { typed = typed.dropLast(1) }
                    Key("0", "pin:0") { if (typed.length < 8) typed += "0" }
                }
            }
        },
        confirmButton = {
            Button(
                onClick = {
                    onEnter(typed)
                    typed = ""
                },
                enabled = typed.length >= 4,
                modifier = Modifier.testTag("pin:enter"),
            ) { Text("Enter") }
        },
        dismissButton = { TextButton(onClick = onCancel, modifier = Modifier.testTag("pin:cancel")) { Text("Cancel") } },
    )
}

@Composable
private fun ApprovalDialog(approval: Approval, error: String?, model: RegisterModel) {
    val approver = approval.approver
    if (approver != null) {
        PinDialog("${approver.name}'s PIN to approve", error, onEnter = model::approve, onCancel = model::cancelApproval)
        return
    }
    AlertDialog(
        onDismissRequest = model::cancelApproval,
        title = { Text("A manager's approval") },
        text = {
            Column {
                Text(approval.reason, modifier = Modifier.testTag("approval:reason"))
                Spacer(Modifier.height(16.dp))
                Text("Who approves?", style = MaterialTheme.typography.titleMedium)
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    for (member in model.approvers()) {
                        Button(onClick = { model.chooseApprover(member) }, modifier = Modifier.testTag("approver:${member.name}")) {
                            Text(member.role?.let { "${member.name} ($it)" } ?: member.name)
                        }
                    }
                }
            }
        },
        confirmButton = {},
        dismissButton = { TextButton(onClick = model::cancelApproval, modifier = Modifier.testTag("approval:cancel")) { Text("Cancel") } },
    )
}

@Composable
private fun TenderDialog(tender: Tender, model: RegisterModel) {
    AlertDialog(
        onDismissRequest = model::closeTender,
        title = { Text((tender.check?.let { "Check ${it.number}, cash: " } ?: "Cash: ") + "${tender.due.text} due") },
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
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                // Part of a check: what was typed is paid toward it, in exact cash.
                if (tender.check != null) {
                    OutlinedButton(onClick = model::payPart, enabled = typed != null, modifier = Modifier.testTag("tender:part")) {
                        Text("Pay ${typed?.text.orEmpty()} of it")
                    }
                }
                Button(onClick = { typed?.let(model::pay) }, enabled = typed != null, modifier = Modifier.testTag("tender:typed")) {
                    Text("Take ${typed?.text.orEmpty()}")
                }
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
                OrderRow(
                    order,
                    elsewhere = model.elsewhere(order),
                    asked = model.askedHere(order),
                    // Only a register that can't reach the hub takes an order on a manager's word
                    // (ADR-0030, decision 6). Found in review: it was offered with the hub up.
                    hubDown = state.sync?.hubReachable == false,
                    onOpen = { model.resume(order) },
                    onReprint = { model.reprint(order) },
                    onTakeOver = { model.takeOver(order) },
                )
            }
        }
    }
}

@Composable
private fun OrderRow(
    order: Ticket,
    elsewhere: Boolean,
    asked: Boolean,
    hubDown: Boolean,
    onOpen: () -> Unit,
    onReprint: () -> Unit,
    onTakeOver: () -> Unit,
) {
    val state = if (order.state == OrderState.OPEN && elsewhere) {
        if (asked) "Asked for" else "On another register"
    } else {
        when (order.state) {
            OrderState.OPEN -> "Open"
            OrderState.CLOSED -> "Paid"
            OrderState.VOIDED -> "Voided"
            OrderState.ABANDONED -> "Dropped"
        }
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
        // Parked on another register: tapping it recalls it through the hub; a manager can take it
        // over when the hub can't be reached.
        if (order.state == OrderState.OPEN && elsewhere && hubDown) {
            TextButton(onClick = onTakeOver, modifier = Modifier.testTag("take-over:${order.order}")) { Text("Take over") }
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
