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
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.compose.material3.ButtonDefaults
import dev.keel.register.Approval
import dev.keel.register.CardPayment
import dev.keel.register.CardSheet
import dev.keel.register.CardState
import dev.keel.register.CardTip
import dev.keel.register.SimulatedAnswer
import dev.keel.register.SimulatedTerminal
import dev.keel.register.declineWords
import dev.keel.register.TicketLine
import dev.keel.register.VOID_REASONS
import dev.keel.register.MarkSheet
import dev.keel.register.backWords
import dev.keel.register.CASH_REASONS
import dev.keel.register.CashEntry
import dev.keel.register.CashPurpose
import dev.keel.register.Drawer
import dev.keel.register.DISCOUNT_REASONS
import dev.keel.register.DiscountSheet
import dev.keel.register.NotesSheet
import dev.keel.register.PRICE_REASONS
import dev.keel.register.PriceSheet
import dev.keel.register.RECEIPT_CODE
import dev.keel.register.RETURN_REASONS
import dev.keel.register.ReturnableLine
import dev.keel.register.TicketDiscount
import dev.keel.register.percentWords
import dev.keel.register.JobKind
import dev.keel.register.JobState
import dev.keel.register.ModifierSheet
import dev.keel.register.OrderState
import dev.keel.register.RegisterModel
import dev.keel.register.say
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.foundation.background
import androidx.compose.runtime.staticCompositionLocalOf
import dev.keel.register.RegisterState
import dev.keel.register.Screen
import dev.keel.register.Tender
import dev.keel.register.Ticket

/** The register: whoever signs in, the menu and the order, and today's orders. */
@Composable
fun RegisterScreen(model: RegisterModel, training: Training? = null) {
    val state by model.state.collectAsStateWithLifecycle()
    // The register's words in the member's language (ADR-0038).
    CompositionLocalProvider(LocalLanguage provides state.language, LocalTraining provides training) {
        RegisterContent(state, model)
    }
}

/** How the screen starts and ends training (ADR-0038). */
class Training(val start: () -> Unit, val end: () -> Unit)

/** Training's controls, where the screen offers them. */
val LocalTraining = staticCompositionLocalOf<Training?> { null }

/** The language the register's words are in, a locale's tag; null for English (ADR-0038). */
val LocalLanguage = staticCompositionLocalOf<String?> { null }

/** [text], one of the register's words, in the member's language. */
@Composable
fun t(text: String): String = say(text, LocalLanguage.current)

@Composable
private fun RegisterContent(state: RegisterState, model: RegisterModel) {
    Scaffold { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            // Nothing a trainee does is real, and the screen never lets them forget (ADR-0038).
            if (state.training) {
                Row(
                    Modifier.fillMaxWidth().background(MaterialTheme.colorScheme.tertiaryContainer).padding(8.dp).testTag("training"),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(t("TRAINING: nothing here is real"), Modifier.weight(1f), style = MaterialTheme.typography.titleMedium)
                    LocalTraining.current?.let { training ->
                        TextButton(onClick = training.end, modifier = Modifier.testTag("training:end")) { Text(t("End training")) }
                    }
                }
            }
            // A PIN pad shows its own refusal.
            state.error?.takeIf { state.pinFor == null && state.approval?.approver == null }?.let { error ->
                Text(
                    t(error),
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
                state.screen == Screen.RETURN -> ReturnPage(state, model)
                state.screen == Screen.KITCHEN -> KitchenPage(state, model)
                state.screen == Screen.STOCK -> StockPage(state, model)
                else -> Register(state, model)
            }
        }
        state.sheet?.let { SheetDialog(it, state.error, model) }
        state.voiding?.let { VoidDialog(it, model) }
        state.mark?.let { MarkDialog(it, state.error, model) }
        state.tender?.let { TenderDialog(it, model) }
        state.card?.let { CardDialog(it, state.error, model) }
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
        Text(t("Who's working?"), style = MaterialTheme.typography.headlineMedium)
        // A kitchen screen needs no one signed in (ADR-0034).
        if (state.stations.isNotEmpty()) {
            TextButton(onClick = model::openKitchen, modifier = Modifier.testTag("kitchen")) { Text(t("Kitchen display")) }
        }
        for (alert in state.alerts) {
            Text(t(alert.words), color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.labelLarge)
        }
        Spacer(Modifier.height(24.dp))
        FlowRow(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            for (member in state.team) {
                Button(onClick = { model.signIn(member) }, modifier = Modifier.width(160.dp).height(88.dp).testTag("member:${member.name}")) {
                    Text(member.name, style = MaterialTheme.typography.titleMedium)
                }
            }
        }
        // Training on this register, apart from its real store (ADR-0038).
        val training = LocalTraining.current
        if (!state.training && training != null) {
            Spacer(Modifier.height(24.dp))
            OutlinedButton(onClick = training.start, modifier = Modifier.testTag("training:start")) { Text(t("Practise in training")) }
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
                // 86: a tap on the menu marks its item out, or puts it back (ADR-0033).
                TextButton(onClick = model::toggleMarking, modifier = Modifier.testTag("mark")) {
                    Text(if (state.marking) "Done marking" else "86")
                }
                TextButton(onClick = model::showOrders, modifier = Modifier.testTag("orders")) { Text(t("Today's orders")) }
                TextButton(onClick = model::showDrawer, modifier = Modifier.testTag("drawer")) { Text(t("Drawer")) }
                TextButton(onClick = model::openStock, modifier = Modifier.testTag("stock")) { Text(t("Stock")) }
                TextButton(onClick = model::showPrinter, modifier = Modifier.testTag("printer")) { Text(t("Printer")) }
                TextButton(onClick = model::signOut, modifier = Modifier.testTag("sign-out")) { Text(t("Sign out")) }
            }
            if (state.pages.isNotEmpty()) {
                PrimaryScrollableTabRow(selectedTabIndex = state.page) {
                    state.pages.forEachIndexed { index, page ->
                        Tab(
                            selected = index == state.page,
                            onClick = { model.showPage(index) },
                            text = { Text(page.name) },
                            modifier = Modifier.testTag("page:${page.name}"),
                        )
                    }
                }
            }
            if (state.marking) {
                Text(
                    "Tap an item to 86 it, or to put it back.",
                    style = MaterialTheme.typography.labelLarge,
                    modifier = Modifier.padding(horizontal = 12.dp, vertical = 4.dp),
                )
            }
            // Card refunds whose latest attempt failed: still owed, tried again on a manager's word (ADR-0035).
            for (owed in state.owedRefunds) {
                Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(horizontal = 12.dp)) {
                    Text(
                        "A card refund of ${owed.amount.text} failed: ${declineWords(owed.failure ?: "declined")}",
                        color = MaterialTheme.colorScheme.error,
                        style = MaterialTheme.typography.labelLarge,
                        modifier = Modifier.weight(1f),
                    )
                    TextButton(onClick = { model.retryRefund(owed) }, modifier = Modifier.testTag("refund:retry:${owed.refund}")) { Text(t("Try again")) }
                }
            }
            // Tickets no screen showed in time, which every device tells its staff (ADR-0034).
            for (alert in state.alerts) {
                Text(
                    alert.words,
                    color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.labelLarge,
                    modifier = Modifier.padding(horizontal = 12.dp, vertical = 2.dp).testTag("alert:${alert.ticket}"),
                )
            }
            if (state.soldOut.isNotEmpty()) {
                Text(
                    "Sold out: " + state.soldOut.joinToString { "${it.name} (${backWords(it.backAt)})" },
                    color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.labelLarge,
                    modifier = Modifier.padding(horizontal = 12.dp, vertical = 4.dp).testTag("sold-out"),
                )
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
                    // A sold-out item stays on the menu, greyed: tapped, the register says so, and
                    // while marking it is put back.
                    val colors = if (button.soldOut) {
                        ButtonDefaults.buttonColors(
                            containerColor = MaterialTheme.colorScheme.surfaceVariant,
                            contentColor = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    } else {
                        ButtonDefaults.buttonColors()
                    }
                    Button(
                        onClick = { model.tap(button) },
                        colors = colors,
                        modifier = Modifier.height(88.dp).testTag("button:${button.variant}"),
                    ) {
                        Column(horizontalAlignment = Alignment.CenterHorizontally) {
                            Text(button.name)
                            Text(
                                if (button.soldOut) {
                                    "Sold out ${backWords(button.backAt)}"
                                } else {
                                    button.price + if (button.required) " •" else ""
                                },
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
    // The whole pane scrolls: on a phone, its lines and buttons are taller than the screen.
    // Found in CI's emulator: Drop order, Split and a line's buttons were out of reach.
    Column(modifier.fillMaxHeight().verticalScroll(rememberScrollState()).padding(16.dp)) {
        val ticket = state.ticket
        if (state.askDrawer) OpenDrawerBanner(model)
        Text(t("Order"), style = MaterialTheme.typography.titleLarge)
        Column(Modifier.fillMaxWidth()) {
            for (line in ticket?.lines.orEmpty()) key(line.line) {
                Column(Modifier.fillMaxWidth().padding(vertical = 4.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text("${line.quantity} × ${line.name}", Modifier.weight(1f))
                        // Sent to the kitchen: it is voided, not changed (ADR-0034).
                        if (line.sent) {
                            Text(
                                "Sent",
                                style = MaterialTheme.typography.labelSmall,
                                modifier = Modifier.padding(horizontal = 6.dp).testTag("sent:${line.line}"),
                            )
                        }
                        Text(line.amount)
                    }
                    for (words in line.modifiers) Text(words, style = MaterialTheme.typography.bodySmall)
                    line.repriced?.let { Text(it, style = MaterialTheme.typography.bodySmall, modifier = Modifier.testTag("repriced:${line.line}")) }
                    line.notes?.let { Text("“$it”", style = MaterialTheme.typography.bodySmall, modifier = Modifier.testTag("notes:${line.line}")) }
                    for (discount in line.discounts) DiscountRow(discount, model)
                    // Wrapped, so every button stays reachable on a narrow pane. Found in review:
                    // the later ones were squeezed to nothing on a phone.
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                        if (line.count != null && !line.sent) {
                            TextButton(onClick = { model.fewer(line) }, modifier = Modifier.testTag("fewer:${line.line}")) { Text("−") }
                            TextButton(onClick = { model.more(line) }, modifier = Modifier.testTag("more:${line.line}")) { Text("+") }
                        }
                        TextButton(onClick = { model.remove(line) }, modifier = Modifier.testTag("remove:${line.line}")) {
                            Text(if (line.sent) "Void" else "Remove")
                        }
                        TextButton(onClick = { model.openDiscount(line) }, modifier = Modifier.testTag("discount:${line.line}")) { Text(t("Discount")) }
                        TextButton(onClick = { model.openPrice(line) }, modifier = Modifier.testTag("price:${line.line}")) { Text(t("Price")) }
                        // A sent line went to the kitchen with its notes. Found in review.
                        if (!line.sent) {
                            TextButton(onClick = { model.openNotes(line) }, modifier = Modifier.testTag("note:${line.line}")) { Text(t("Note")) }
                        }
                    }
                    // A line moves to a check of its own, or to another open check of a split order.
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                        for (check in ticket?.checks.orEmpty().filter { it.open && it.lines.none { on -> on.line == line.line } }) {
                            TextButton(onClick = { model.moveLine(line, check.check) }, modifier = Modifier.testTag("move:${check.number}:${line.line}")) {
                                Text("To check ${check.number}")
                            }
                        }
                        TextButton(onClick = { model.moveLine(line, null) }, modifier = Modifier.testTag("move:new:${line.line}")) {
                            Text(t("Own check"))
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
                        if (model.terminal != null) {
                            TextButton(onClick = { model.openCard(check) }, modifier = Modifier.testTag("card:check:${check.number}")) {
                                Text(t("Card"))
                            }
                        }
                    }
                }
            }
            Spacer(Modifier.height(8.dp))
            // The kitchen gets what is pending when it is sent, or when it is paid (ADR-0034).
            if (state.stations.isNotEmpty() && ticket.lines.any { !it.sent }) {
                OutlinedButton(onClick = model::send, modifier = Modifier.fillMaxWidth().testTag("send")) {
                    Text(t("Send to the kitchen"))
                }
            }
            if (ticket.checks.isEmpty()) {
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Button(
                        onClick = { model.openTender() },
                        enabled = ticket.lines.isNotEmpty(),
                        modifier = Modifier.weight(1f).testTag("pay"),
                    ) { Text("Cash ${ticket.cashDue}") }
                    if (model.terminal != null) {
                        Button(
                            onClick = { model.openCard() },
                            enabled = ticket.lines.isNotEmpty(),
                            modifier = Modifier.weight(1f).testTag("card"),
                        ) { Text(t("Card")) }
                    }
                }
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
            ) { Text(t("Discount the order")) }
            OutlinedButton(onClick = model::abandon, modifier = Modifier.fillMaxWidth().testTag("abandon")) { Text(t("Drop order")) }
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
        Text(t("No drawer is open."), Modifier.weight(1f), style = MaterialTheme.typography.bodySmall)
        TextButton(onClick = { model.beginCash(CashPurpose.FLOAT) }, modifier = Modifier.testTag("drawer:open")) { Text(t("Open it")) }
        TextButton(onClick = model::dismissDrawer, modifier = Modifier.testTag("drawer:later")) { Text(t("Not now")) }
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
        CashPurpose.NO_SALE -> "Open with no sale"
    }
    val noSale = entry.purpose == CashPurpose.NO_SALE
    val moving = entry.purpose.moves || noSale
    AlertDialog(
        onDismissRequest = model::cancelCash,
        title = { Text(title) },
        text = {
            Column(Modifier.heightIn(max = 520.dp).verticalScroll(rememberScrollState())) {
                if (moving) {
                    Text(t("Why"), style = MaterialTheme.typography.titleSmall)
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        for (reason in CASH_REASONS) {
                            FilterChip(
                                selected = entry.reason == reason.code,
                                onClick = { model.cashReason(reason.code) },
                                label = { Text(t(reason.words)) },
                                modifier = Modifier.testTag("cash-reason:${reason.code}"),
                            )
                        }
                    }
                }
                if (entry.purpose == CashPurpose.COUNT) {
                    // Blind: what the drawer should hold isn't shown until the count is in.
                    Text(t("Count what is in the drawer, and type it."), style = MaterialTheme.typography.bodySmall)
                }
                if (!noSale) {
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
                }
                error?.let { Text(it, color = MaterialTheme.colorScheme.error, modifier = Modifier.testTag("cash:error")) }
            }
        },
        confirmButton = {
            Button(onClick = model::confirmCash, modifier = Modifier.testTag("cash:confirm")) {
                Text(
                    when (entry.purpose) {
                        CashPurpose.COUNT -> "Close the drawer"
                        CashPurpose.NO_SALE -> "Open the drawer"
                        else -> "Done"
                    },
                )
            }
        },
        dismissButton = { TextButton(onClick = model::cancelCash, modifier = Modifier.testTag("cash:cancel")) { Text(t("Cancel")) } },
    )
}

/** The drawer: what moved through it, its movements, its close, and the day's reports. */
@Composable
private fun DrawerPage(state: RegisterState, model: RegisterModel) {
    Column(Modifier.fillMaxSize().padding(16.dp).verticalScroll(rememberScrollState())) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(t("Drawer"), Modifier.weight(1f), style = MaterialTheme.typography.titleLarge)
            TextButton(onClick = model::showRegister, modifier = Modifier.testTag("back")) { Text(t("Back")) }
        }
        val drawer = state.drawer
        if (drawer == null) {
            Text(t("No drawer is open."), modifier = Modifier.testTag("drawer:none"))
            Button(onClick = { model.beginCash(CashPurpose.FLOAT) }, modifier = Modifier.testTag("drawer:open")) { Text(t("Open the drawer")) }
        } else {
            DrawerFigures(drawer)
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedButton(onClick = { model.beginCash(CashPurpose.PAID_IN) }, modifier = Modifier.testTag("drawer:in")) { Text(t("Pay in")) }
                OutlinedButton(onClick = { model.beginCash(CashPurpose.PAID_OUT) }, modifier = Modifier.testTag("drawer:out")) { Text(t("Pay out")) }
                OutlinedButton(onClick = { model.beginCash(CashPurpose.DROP) }, modifier = Modifier.testTag("drawer:drop")) { Text(t("Drop")) }
                OutlinedButton(onClick = { model.beginCash(CashPurpose.NO_SALE) }, modifier = Modifier.testTag("drawer:no-sale")) { Text(t("No sale")) }
                Button(onClick = { model.beginCash(CashPurpose.COUNT) }, modifier = Modifier.testTag("drawer:close")) { Text(t("Close the drawer")) }
            }
        }
        state.closedDrawer?.let { closed ->
            HorizontalDivider(Modifier.padding(vertical = 8.dp))
            Text(t("Last close"), style = MaterialTheme.typography.titleMedium)
            DrawerFigures(closed)
        }
        HorizontalDivider(Modifier.padding(vertical = 8.dp))
        Text(t("Today's reports"), style = MaterialTheme.typography.titleMedium)
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedButton(onClick = { model.showReport(z = false) }, modifier = Modifier.testTag("report:x")) { Text(t("X report")) }
            // The Z waits for the drawer to close: not offered until it is. Found in review.
            OutlinedButton(onClick = { model.showReport(z = true) }, enabled = drawer == null, modifier = Modifier.testTag("report:z")) { Text(t("Z report")) }
        }
        state.report?.let { report ->
            Text(if (report.z) "Z report, ${report.date}" else "X report, ${report.date}", style = MaterialTheme.typography.titleSmall)
            ReportRow("Orders", report.orders.toString())
            ReportRow("Gross", report.gross)
            ReportRow("Discounts", report.discounts)
            ReportRow("Comps", report.comps)
            ReportRow("Net", report.net)
            ReportRow("Tax", report.tax)
            ReportRow("Total", report.total, Modifier.testTag("report:total"))
            for ((tender, amount) in report.tenders) ReportRow(tender, amount)
            ReportRow("Voided orders", report.voided.toString())
            report.withoutDrawer?.let { (words, amount) -> ReportRow(words, amount) }
            for ((words, amount) in report.refunds) ReportRow(words, amount)
            report.refundedWithoutDrawer?.let { (words, amount) -> ReportRow(words, amount) }
            report.cardRefundsOwed?.let { (words, amount) -> ReportRow(words, amount) }
            if (report.wasted.isNotEmpty()) {
                Text(t("Wasted"), style = MaterialTheme.typography.titleSmall)
                for ((what, quantity) in report.wasted) ReportRow(what, quantity)
            }
            if (report.counts.isNotEmpty()) {
                Text(t("Counts"), style = MaterialTheme.typography.titleSmall)
                for ((what, variance) in report.counts) ReportRow(what, variance)
            }
            if (report.tips.isNotEmpty()) {
                Text(t("Tips"), style = MaterialTheme.typography.titleSmall)
                for ((who, amount) in report.tips) ReportRow(who, amount)
            }
            // Each drawer of the day, with its over or short once counted. Found in review: only
            // the printed report had them.
            report.drawers.forEachIndexed { index, each ->
                Text("Drawer ${index + 1}", style = MaterialTheme.typography.titleSmall)
                DrawerFigures(each)
                if (each.open) Text(t("Still open"), style = MaterialTheme.typography.bodySmall)
            }
            Button(onClick = { model.printReport(report.z) }, modifier = Modifier.testTag("report:print")) { Text(t("Print")) }
        }
    }
}

@Composable
private fun DrawerFigures(drawer: Drawer) {
    // Blind while open: what moved through it shows once it is counted (ADR-0031).
    ReportRow("Float", drawer.float)
    drawer.taken?.let { ReportRow("Cash sales", it, Modifier.testTag("closed:taken")) }
    drawer.paidIn?.let { ReportRow("Paid in", it) }
    drawer.paidOut?.let { ReportRow("Paid out", it) }
    drawer.dropped?.let { ReportRow("Dropped", it) }
    drawer.refunded?.let { ReportRow("Refunds", it) }
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
                Text(t("Why"), style = MaterialTheme.typography.titleSmall)
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    for (reason in PRICE_REASONS) {
                        FilterChip(
                            selected = sheet.reason == reason.code,
                            onClick = { model.priceReason(reason.code) },
                            label = { Text(t(reason.words)) },
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
        dismissButton = { TextButton(onClick = model::closePrice, modifier = Modifier.testTag("price:cancel")) { Text(t("Cancel")) } },
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
                label = { Text(t("For the kitchen and the receipt")) },
                modifier = Modifier.fillMaxWidth().testTag("notes:text"),
            )
        },
        confirmButton = {
            Button(onClick = { model.saveNotes(text) }, modifier = Modifier.testTag("notes:save")) { Text(t("Keep")) }
        },
        dismissButton = { TextButton(onClick = model::closeNotes, modifier = Modifier.testTag("notes:cancel")) { Text(t("Cancel")) } },
    )
}

/** The most characters a note holds, as the kernel keeps them. */
private const val MAX_NOTE = 500

/** A discount on the ticket: its words, what it took, and a way to take it off. */
@Composable
private fun DiscountRow(discount: TicketDiscount, model: RegisterModel) {
    Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.testTag("discounted:${discount.discount}")) {
        Text(t(discount.words), Modifier.weight(1f), style = MaterialTheme.typography.bodySmall)
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
                Text(t("Why"), style = MaterialTheme.typography.titleSmall)
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    for (reason in DISCOUNT_REASONS) {
                        FilterChip(
                            selected = sheet.reason == reason.code,
                            onClick = { model.discountReason(reason.code) },
                            label = { Text(t(reason.words)) },
                            modifier = Modifier.testTag("reason:${reason.code}"),
                        )
                    }
                }
                Spacer(Modifier.height(8.dp))
                Text(t("How much"), style = MaterialTheme.typography.titleSmall)
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
        dismissButton = { TextButton(onClick = model::closeDiscount, modifier = Modifier.testTag("discount:cancel")) { Text(t("Cancel")) } },
    )
}

@Composable
private fun SheetDialog(sheet: ModifierSheet, error: String?, model: RegisterModel) {
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
                                // Sold out, it isn't chosen (ADR-0033).
                                enabled = !option.soldOut || count > 0,
                                label = {
                                    val price = if (option.soldOut) "sold out" else option.price
                                    Text(option.name + (if (count > 1) " ×$count" else "") + " " + price)
                                },
                                modifier = Modifier.testTag("modifier:${option.modifier}"),
                            )
                        }
                    }
                }
                // Its refusals show on it, not behind it. Found in review.
                error?.let { Text(it, color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(top = 8.dp).testTag("sheet:error")) }
                Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(top = 12.dp)) {
                    Text(t("How many"), Modifier.weight(1f))
                    TextButton(onClick = { model.sheetQuantity(sheet.quantity - 1) }) { Text("−") }
                    Text(sheet.quantity.toString(), modifier = Modifier.testTag("sheet:quantity"))
                    TextButton(onClick = { model.sheetQuantity(sheet.quantity + 1) }) { Text("+") }
                }
            }
        },
        confirmButton = {
            Button(onClick = model::addFromSheet, enabled = sheet.complete, modifier = Modifier.testTag("sheet:add")) { Text(t("Add")) }
        },
        dismissButton = { TextButton(onClick = model::closeSheet) { Text(t("Cancel")) } },
    )
}

/** Why a line sent to the kitchen is voided (ADR-0034): it won't be made or charged. */
@Composable
private fun VoidDialog(line: TicketLine, model: RegisterModel) {
    AlertDialog(
        onDismissRequest = model::closeVoid,
        title = { Text("Void ${line.name}") },
        text = {
            Column {
                Text(t("It was sent to the kitchen: voided, it won't be made or charged."))
                for (reason in VOID_REASONS) {
                    TextButton(onClick = { model.voidLine(reason.code) }, modifier = Modifier.testTag("void:${reason.code}")) {
                        Text(t(reason.words))
                    }
                }
            }
        },
        confirmButton = {},
        dismissButton = { TextButton(onClick = model::closeVoid) { Text(t("Keep it")) } },
    )
}

/** The 86 sheet (ADR-0033): the item, and each modifier it offers, 86'd for the day or until put back, or put back. */
@Composable
private fun MarkDialog(sheet: MarkSheet, error: String?, model: RegisterModel) {
    AlertDialog(
        onDismissRequest = model::closeMark,
        title = { Text("86: ${sheet.button.name}") },
        text = {
            Column(Modifier.heightIn(max = 480.dp).verticalScroll(rememberScrollState())) {
                MarkRow(sheet.button.variant, sheet.button.name, false, sheet.button.soldOut, sheet.button.backAt, sheet.working, model)
                if (sheet.modifiers.isNotEmpty()) {
                    Text(t("Its modifiers"), style = MaterialTheme.typography.titleSmall, modifier = Modifier.padding(top = 12.dp))
                }
                for (option in sheet.modifiers) {
                    MarkRow(option.modifier, option.name, true, option.soldOut, option.backAt, sheet.working, model)
                }
                error?.let { Text(it, color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(top = 8.dp).testTag("mark:error")) }
            }
        },
        confirmButton = { TextButton(onClick = model::closeMark, modifier = Modifier.testTag("mark:done")) { Text(t("Done")) } },
    )
}

@Composable
private fun MarkRow(id: String, name: String, modifier: Boolean, soldOut: Boolean, backAt: String?, working: Boolean, model: RegisterModel) {
    Row(Modifier.fillMaxWidth().padding(vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text(name)
            if (soldOut) Text("Sold out ${backWords(backAt)}", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.error)
        }
        if (soldOut) {
            OutlinedButton(onClick = { model.putBack(id, modifier) }, enabled = !working, modifier = Modifier.testTag("mark:$id:put-back")) {
                Text(t("Put back"))
            }
        } else {
            OutlinedButton(onClick = { model.eightySix(id, modifier, untilPutBack = false) }, enabled = !working, modifier = Modifier.testTag("mark:$id:day")) {
                Text(t("86 today"))
            }
            Spacer(Modifier.width(8.dp))
            OutlinedButton(onClick = { model.eightySix(id, modifier, untilPutBack = true) }, enabled = !working, modifier = Modifier.testTag("mark:$id:back")) {
                Text(t("Until put back"))
            }
        }
    }
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
            ) { Text(t("Enter")) }
        },
        dismissButton = { TextButton(onClick = onCancel, modifier = Modifier.testTag("pin:cancel")) { Text(t("Cancel")) } },
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
        title = { Text(t("A manager's approval")) },
        text = {
            Column {
                Text(approval.reason, modifier = Modifier.testTag("approval:reason"))
                Spacer(Modifier.height(16.dp))
                Text(t("Who approves?"), style = MaterialTheme.typography.titleMedium)
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
        dismissButton = { TextButton(onClick = model::cancelApproval, modifier = Modifier.testTag("approval:cancel")) { Text(t("Cancel")) } },
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
        dismissButton = { TextButton(onClick = model::closeTender) { Text(t("Cancel")) } },
    )
}

/**
 * Paying by card (ADR-0035): the tips first, where the register asks for them, "No tip" and
 * "Custom" as prominent as each preset, shown as a percentage and an amount (H-10); then the
 * payment as the terminal answers it. A simulated terminal shows the customer's choices.
 */
@Composable
private fun CardDialog(sheet: CardSheet, error: String?, model: RegisterModel) {
    val payment = sheet.payment
    AlertDialog(
        onDismissRequest = {},
        title = { Text((sheet.check?.let { "Check ${it.number}, card" } ?: "Card")) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                // Refusals show here, above the dialog's scrim. Found in review.
                error?.let { Text(it, color = MaterialTheme.colorScheme.error, modifier = Modifier.testTag("card:error")) }
                when {
                    sheet.asksTip && sheet.custom -> CustomTip(sheet, model)
                    sheet.asksTip -> {
                        Text(t("Add a tip?"), style = MaterialTheme.typography.titleMedium)
                        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                            OutlinedButton(onClick = { model.chooseTip(CardTip.None) }, enabled = !sheet.working, modifier = Modifier.testTag("tip:none")) {
                                Text(t("No tip"))
                            }
                            for (offer in sheet.offers.presets) {
                                OutlinedButton(
                                    onClick = { model.chooseTip(CardTip.Amount(offer.amount.minor)) },
                                    enabled = !sheet.working,
                                    modifier = Modifier.testTag("tip:${offer.percent}"),
                                ) { Text("${percentWords(offer.percent)} · ${offer.amount.text}") }
                            }
                            OutlinedButton(onClick = model::customTip, enabled = !sheet.working, modifier = Modifier.testTag("tip:custom")) {
                                Text(t("Custom"))
                            }
                        }
                        Text("Tips are on ${sheet.offers.base.text}, before tax.", style = MaterialTheme.typography.labelMedium)
                    }
                    payment == null -> Text(t("Starting the payment…"))
                    else -> CardPaymentState(payment, model)
                }
            }
        },
        confirmButton = {
            when (payment?.state) {
                CardState.Waiting -> TextButton(onClick = model::cancelCard, modifier = Modifier.testTag("card:cancel")) { Text(t("Cancel the card")) }
                is CardState.Declined -> Button(onClick = model::cardAgain, modifier = Modifier.testTag("card:again")) { Text(t("Try a card again")) }
                is CardState.Approved -> Button(onClick = model::cardAgain, modifier = Modifier.testTag("card:again")) { Text(t("Another card for the rest")) }
                else -> {}
            }
        },
        dismissButton = {
            // A payment waiting for the terminal, or checked, stays with it: the sheet closes, and
            // the check takes no other payment until it is answered.
            TextButton(onClick = model::closeCard, modifier = Modifier.testTag("card:close")) { Text(t("Close")) }
        },
    )
}

@Composable
private fun CustomTip(sheet: CardSheet, model: RegisterModel) {
    Text(sheet.typed?.text ?: "Type the tip", style = MaterialTheme.typography.headlineSmall, modifier = Modifier.testTag("tip:typed"))
    for (row in listOf(listOf(1, 2, 3), listOf(4, 5, 6), listOf(7, 8, 9))) {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            for (digit in row) Key(digit.toString(), "tip:digit:$digit") { model.typeTipDigit(digit) }
        }
    }
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Key("⌫", "tip:erase") { model.eraseTipDigit() }
        Key("0", "tip:digit:0") { model.typeTipDigit(0) }
        val typed = sheet.typed
        Button(onClick = { typed?.let { model.chooseTip(CardTip.Amount(it.minor)) } }, enabled = typed != null && !sheet.working, modifier = Modifier.testTag("tip:add")) {
            Text("Add ${typed?.text.orEmpty()}")
        }
    }
}

@Composable
private fun CardPaymentState(payment: CardPayment, model: RegisterModel) {
    when (val state = payment.state) {
        CardState.Waiting -> {
            Text("Present the card on the terminal: ${payment.amount.text}", modifier = Modifier.testTag("card:waiting"))
            // The simulated terminal stands in for the customer until a processor is chosen.
            val simulated = model.terminal as? SimulatedTerminal
            if (simulated != null) {
                val asked by simulated.waitingFor.collectAsState()
                if (asked != null) {
                    Text(t("Simulated terminal"), style = MaterialTheme.typography.labelLarge)
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        for ((answer, words) in listOf(
                            SimulatedAnswer.APPROVE to "Approve",
                            SimulatedAnswer.APPROVE_PART to "Approve half",
                            SimulatedAnswer.DECLINE to "Decline",
                            SimulatedAnswer.NO_ANSWER to "No answer",
                        )) {
                            OutlinedButton(onClick = { simulated.answer(answer) }, modifier = Modifier.testTag("terminal:${answer.name}")) { Text(words) }
                        }
                    }
                }
            }
        }
        CardState.Checking -> {
            Text(t("Checking with the terminal: it didn't say whether the card was charged."), modifier = Modifier.testTag("card:checking"))
            Text(t("If the terminal can't be reached, a manager says from its own record:"), style = MaterialTheme.typography.labelMedium)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedButton(onClick = { model.settleCard(charged = true) }, modifier = Modifier.testTag("card:charged")) { Text(t("It was charged")) }
                OutlinedButton(onClick = { model.settleCard(charged = false) }, modifier = Modifier.testTag("card:not-charged")) { Text(t("It wasn't")) }
            }
        }
        CardState.Elsewhere -> Text(t("Another register's terminal has this payment."))
        is CardState.Approved -> {
            val tip = state.tip?.let { " with a ${it.text} tip" }.orEmpty()
            val card = payment.card?.let { " on $it" }.orEmpty()
            val how = if (state.byHand) ", settled by hand" else ""
            Text("Approved: ${state.applied.text}$tip$card$how", modifier = Modifier.testTag("card:approved"))
            if (state.applied.minor < payment.amount.minor) {
                Text(t("Approved in part: the rest is still due, by card or in cash."), color = MaterialTheme.colorScheme.error)
            }
        }
        is CardState.Declined -> Text("Declined: ${declineWords(state.reason)}", color = MaterialTheme.colorScheme.error, modifier = Modifier.testTag("card:declined"))
    }
}

@Composable
private fun Key(label: String, tag: String, onClick: () -> Unit) {
    OutlinedButton(onClick = onClick, modifier = Modifier.width(72.dp).testTag(tag)) { Text(label) }
}

@Composable
private fun Orders(state: RegisterState, model: RegisterModel) {
    Column(Modifier.fillMaxSize().padding(16.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(t("Today's orders"), Modifier.weight(1f), style = MaterialTheme.typography.titleLarge)
            TextButton(onClick = model::showRegister, modifier = Modifier.testTag("back")) { Text(t("Back")) }
        }
        // A paid order found from its receipt's code, to return (ADR-0032).
        var code by rememberSaveable { mutableStateOf("") }
        Row(verticalAlignment = Alignment.CenterVertically) {
            OutlinedTextField(
                value = code,
                onValueChange = { code = it.take(6) },
                label = { Text(t("Receipt code")) },
                singleLine = true,
                modifier = Modifier.weight(1f).testTag("return:code"),
            )
            // Six characters, as a receipt prints: found in review, a shorter code opened whatever
            // order it ended.
            TextButton(
                onClick = { model.findReturn(code) },
                enabled = code.trim().length == RECEIPT_CODE,
                modifier = Modifier.testTag("return:find"),
            ) { Text(t("Find")) }
        }
        // More than one paid order has the code: the cashier chooses.
        state.matches?.let { matches ->
            Text(t("Orders with that code"), style = MaterialTheme.typography.titleSmall)
            for (match in matches) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(match.lines.joinToString { "${it.quantity} × ${it.name}" }, Modifier.weight(1f))
                    Text(match.total)
                    TextButton(onClick = { model.beginReturn(match) }, modifier = Modifier.testTag("match:${match.order}")) { Text(t("Return")) }
                }
            }
        }
        if (state.orders.isEmpty()) Text(t("None yet today."))
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
                    onReturn = { model.beginReturn(order) },
                    onTakeOver = { model.takeOver(order) },
                )
            }
        }
    }
}

/**
 * A return (ADR-0032): each line of the paid order with what is left of it, picked one at a time,
 * or whole for one sold by measure; a reason; and the refund, with what to hand back.
 */
@Composable
private fun ReturnPage(state: RegisterState, model: RegisterModel) {
    val sheet = state.returning ?: return
    Column(Modifier.fillMaxSize().padding(16.dp).verticalScroll(rememberScrollState())) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(t("Return"), Modifier.weight(1f), style = MaterialTheme.typography.titleLarge)
            TextButton(onClick = model::closeReturn, modifier = Modifier.testTag("back")) { Text(t("Back")) }
        }
        val toCard = sheet.returnable.cards.firstOrNull { it.payment == sheet.toCard }
        val cash = sheet.returnable.cashOnly || sheet.returnable.cashLeft != null
        val refundable = cash || toCard != null
        // Where it goes: cash, up to what the order took, or one of its cards (ADR-0035).
        if (sheet.returnable.cards.isNotEmpty()) {
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                if (cash) {
                    val left = sheet.returnable.cashLeft?.let { " (up to ${it.text})" }.orEmpty()
                    FilterChip(selected = sheet.toCard == null, onClick = { model.refundTo(null) }, label = { Text("Cash$left") }, modifier = Modifier.testTag("refund-to:cash"))
                }
                for (card in sheet.returnable.cards) {
                    FilterChip(
                        selected = sheet.toCard == card.payment,
                        onClick = { model.refundTo(card.payment) },
                        label = { Text("${card.card ?: "Card"} (up to ${card.left.text})") },
                        modifier = Modifier.testTag("refund-to:${card.payment}"),
                    )
                }
            }
        } else if (!refundable) {
            Text(t("This register can't refund this order: a payment of it isn't known here yet, or a card settled by hand paid it."))
        }
        val picking = refundable && !sheet.refunding
        for (line in sheet.returnable.lines) ReturnRow(line, sheet.picks[line.line], line.line in sheet.picks, picking, model)
        Text("Refunded so far: ${sheet.returnable.refunded}", style = MaterialTheme.typography.bodySmall)
        Text(t("Why"), style = MaterialTheme.typography.titleSmall)
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            for (reason in RETURN_REASONS) {
                FilterChip(
                    selected = sheet.reason == reason.code,
                    onClick = { model.returnReason(reason.code) },
                    label = { Text(t(reason.words)) },
                    modifier = Modifier.testTag("return-reason:${reason.code}"),
                )
            }
        }
        // Back to stock, unless it can't be sold again: then it is wasted (ADR-0036). It changes
        // nothing for an item whose stock isn't tracked.
        Row(verticalAlignment = Alignment.CenterVertically) {
            FilterChip(
                selected = sheet.restock,
                onClick = { model.returnToStock(!sheet.restock) },
                // The chip reads the same chosen or not; its tick says which. Found in review.
                label = { Text(t("Back to stock")) },
                modifier = Modifier.testTag("return:restock"),
            )
            if (!sheet.restock) Text(t("  Not back to stock: recorded wasted"), style = MaterialTheme.typography.bodySmall)
        }
        Button(
            onClick = model::confirmReturn,
            enabled = sheet.picks.isNotEmpty() && sheet.reason != null && refundable && !sheet.refunding,
            modifier = Modifier.fillMaxWidth().testTag("return:refund"),
        ) { Text(if (sheet.toCard != null) "Refund to the card" else "Refund in cash") }
        state.refund?.let { refund ->
            HorizontalDivider(Modifier.padding(vertical = 8.dp))
            for ((what, amount) in refund.lines) ReportRow(what, amount)
            ReportRow("Refunded", refund.amount, Modifier.testTag("return:amount"))
            if (refund.toCard) {
                Text("Sent to ${refund.card ?: "the card"}: the terminal refunds it", style = MaterialTheme.typography.titleMedium, modifier = Modifier.testTag("return:card"))
            } else {
                Text("Hand back ${refund.handed}", style = MaterialTheme.typography.titleMedium, modifier = Modifier.testTag("return:handed"))
            }
        }
    }
}

@Composable
private fun ReturnRow(line: ReturnableLine, picked: Long?, isPicked: Boolean, picking: Boolean, model: RegisterModel) {
    Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth().testTag("return-line:${line.line}")) {
        Column(Modifier.weight(1f)) {
            Text(line.name)
            Text("${line.leftText} left of ${line.charged}", style = MaterialTheme.typography.bodySmall)
        }
        TextButton(onClick = { model.pickFewer(line) }, enabled = picking && isPicked, modifier = Modifier.testTag("return-fewer:${line.line}")) { Text("−") }
        Text(
            when {
                !isPicked -> "0"
                picked == null -> "All"
                else -> picked.toString()
            },
            modifier = Modifier.testTag("return-picked:${line.line}"),
        )
        TextButton(onClick = { model.pickMore(line) }, enabled = picking && line.anyLeft, modifier = Modifier.testTag("return-more:${line.line}")) { Text("+") }
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
    onReturn: () -> Unit,
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
            TextButton(onClick = onReprint, modifier = Modifier.testTag("reprint:${order.order}")) { Text(t("Reprint")) }
            // Not on one another register owns, which refuses it. Found in review.
            if (!elsewhere) {
                TextButton(onClick = onReturn, modifier = Modifier.testTag("return:${order.order}")) { Text(t("Return")) }
            }
        }
        // Parked on another register: tapping it recalls it through the hub; a manager can take it
        // over when the hub can't be reached.
        if (order.state == OrderState.OPEN && elsewhere && hubDown) {
            TextButton(onClick = onTakeOver, modifier = Modifier.testTag("take-over:${order.order}")) { Text(t("Take over")) }
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
            Text(t("Printer"), Modifier.weight(1f), style = MaterialTheme.typography.titleLarge)
            TextButton(onClick = model::refreshJobs, modifier = Modifier.testTag("refresh")) { Text(t("Refresh")) }
            TextButton(onClick = model::showRegister, modifier = Modifier.testTag("back")) { Text(t("Back")) }
        }
        Text(t("Where the receipt printer listens on the store's network."))
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            OutlinedTextField(host, { host = it }, label = { Text(t("Address")) }, singleLine = true, modifier = Modifier.weight(1f).testTag("printer:host"))
            OutlinedTextField(port, { port = it }, label = { Text(t("Port")) }, singleLine = true, modifier = Modifier.width(120.dp).testTag("printer:port"))
            Button(onClick = { model.setPrinter(host, port) }, modifier = Modifier.testTag("printer:save")) { Text(t("Save")) }
        }
        Spacer(Modifier.height(16.dp))
        Text(t("Jobs"), style = MaterialTheme.typography.titleMedium)
        if (state.jobs.isEmpty()) Text(t("Nothing waiting to print."))
        LazyColumn {
            items(state.jobs, key = { it.job }) { job ->
                Row(Modifier.fillMaxWidth().padding(vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                    val kind = when (job.kind) {
                        JobKind.RECEIPT -> "Receipt"
                        JobKind.DRAWER -> "Cash drawer"
                        JobKind.KITCHEN -> "Kitchen ticket"
                    }
                    val where = when (job.state) {
                        JobState.QUEUED -> if (job.attempts == 0) "Waiting" else "Waiting, tried ${job.attempts}×"
                        JobState.SENDING -> "Printing"
                        JobState.UNCONFIRMED -> "Did it print?"
                    }
                    Text("$kind: $where", Modifier.weight(1f))
                    if (job.state == JobState.UNCONFIRMED) {
                        TextButton(onClick = { model.resolve(job, printed = true) }) { Text(t("It printed")) }
                        TextButton(onClick = { model.resolve(job, printed = false) }) { Text(t("Print again")) }
                    }
                }
                HorizontalDivider()
            }
        }
    }
}
