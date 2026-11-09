package dev.keel.android

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.ScrollableTabRow
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import dev.keel.register.MenuButton
import dev.keel.register.RegisterModel
import dev.keel.register.RegisterState

/** The register: the menu's pages of buttons, and beside them the order being rung. */
@Composable
fun RegisterScreen(model: RegisterModel) {
    val state by model.state.collectAsStateWithLifecycle()
    Scaffold { padding ->
        if (state.loading && state.error == null) {
            Column(
                Modifier.fillMaxSize().padding(padding),
                verticalArrangement = Arrangement.Center,
                horizontalAlignment = Alignment.CenterHorizontally,
            ) { CircularProgressIndicator() }
            return@Scaffold
        }
        Row(Modifier.fillMaxSize().padding(padding)) {
            Menu(state, onPage = model::showPage, onTap = model::tap, modifier = Modifier.weight(2f))
            Ticket(state, onPay = model::payExact, modifier = Modifier.weight(1f))
        }
    }
}

@Composable
private fun Menu(
    state: RegisterState,
    onPage: (Int) -> Unit,
    onTap: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(modifier.fillMaxHeight()) {
        if (state.pages.isNotEmpty()) {
            ScrollableTabRow(selectedTabIndex = state.page) {
                state.pages.forEachIndexed { index, page ->
                    Tab(selected = index == state.page, onClick = { onPage(index) }, text = { Text(page.name) })
                }
            }
        }
        val buttons: List<MenuButton> = state.pages.getOrNull(state.page)?.buttons.orEmpty()
        LazyVerticalGrid(
            columns = GridCells.Adaptive(minSize = 140.dp),
            modifier = Modifier.fillMaxSize().padding(8.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            items(buttons, key = { it.variant }) { button ->
                Button(
                    onClick = { onTap(button.variant) },
                    modifier = Modifier.height(88.dp).testTag("button:${button.variant}"),
                ) {
                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        Text(button.name)
                        Text(button.price, style = MaterialTheme.typography.labelSmall)
                    }
                }
            }
        }
    }
}

@Composable
private fun Ticket(state: RegisterState, onPay: () -> Unit, modifier: Modifier = Modifier) {
    Column(modifier.fillMaxHeight().padding(16.dp)) {
        val ticket = state.ticket
        Text("Order", style = MaterialTheme.typography.titleLarge)
        LazyColumn(Modifier.weight(1f).fillMaxWidth()) {
            items(ticket?.lines.orEmpty(), key = { it.line }) { line ->
                Row(Modifier.fillMaxWidth().padding(vertical = 4.dp)) {
                    Text("${line.quantity} × ${line.name}", Modifier.weight(1f))
                    Text(line.amount)
                }
            }
        }
        state.error?.let { Text(it, color = MaterialTheme.colorScheme.error, modifier = Modifier.testTag("error")) }
        state.change?.let { Text("Change: $it", modifier = Modifier.testTag("change")) }
        if (ticket != null) {
            Text("Total ${ticket.total}", style = MaterialTheme.typography.titleMedium, modifier = Modifier.testTag("total"))
            Spacer(Modifier.height(8.dp))
            Button(onClick = onPay, modifier = Modifier.fillMaxWidth().testTag("pay")) {
                Text("Cash ${ticket.cashDue}")
            }
        }
    }
}
