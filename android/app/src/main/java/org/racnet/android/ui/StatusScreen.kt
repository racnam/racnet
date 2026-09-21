package org.racnet.android.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import org.racnet.android.mesh.PeerInfo
import org.racnet.android.messages.BoardMessage
import uniffi.racnet_core.EntryView
import java.text.DateFormat
import java.util.Date

val TEST_ENTRY_SIZES = listOf(100, 1_024, 10_240, 49_152)

@Composable
fun StatusScreen(
    running: Boolean,
    fingerprintHex: String,
    authorKey: ByteArray,
    peers: List<PeerInfo>,
    entries: List<EntryView>,
    error: String?,
    sending: Boolean,
    draft: String,
    onDraftChanged: (String) -> Unit,
    onToggleService: (Boolean) -> Unit,
    onCreateEntry: (Int) -> Unit,
    onSend: () -> Unit,
    onShowDiagnostics: () -> Unit,
) {
    var showTools by rememberSaveable { mutableStateOf(false) }
    val messages = remember(entries) {
        entries.mapNotNull { entry ->
            BoardMessage.decode(entry.kind, entry.payload)?.let { entry to it }
        }.asReversed()
    }
    Column(
        Modifier.fillMaxSize().safeDrawingPadding().imePadding().padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            Column(Modifier.weight(1f)) {
                Text("Racnet", style = MaterialTheme.typography.headlineMedium)
                Text(if (running) "Mesh on · ${peers.size} nearby peers" else "Offline · saved on this phone")
            }
            Switch(checked = running, onCheckedChange = onToggleService)
        }
        Text(
            "Public nearby board. Everyone on the mesh can read and relay messages. " +
                "Delivery waits for contact; it is not guaranteed.",
            style = MaterialTheme.typography.bodySmall,
        )
        error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            TextButton(onClick = { showTools = !showTools }) { Text("Peers & tools") }
            TextButton(onClick = onShowDiagnostics) { Text("Diagnostics") }
        }
        if (showTools) {
            SelectionContainer {
                Text("Your peer fingerprint: $fingerprintHex", style = MaterialTheme.typography.bodySmall)
            }
            peers.forEach {
                Text("Peer ${it.fingerprintHex.take(16)}…", fontFamily = FontFamily.Monospace)
            }
            Text("${entries.size} stored entries · test data is hidden from the board")
            Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                TEST_ENTRY_SIZES.forEach { size ->
                    TextButton(onClick = { onCreateEntry(size) }, enabled = !sending) {
                        Text(sizeLabel(size))
                    }
                }
            }
        }
        LazyColumn(Modifier.weight(1f).fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            if (messages.isEmpty()) {
                item {
                    Text("No messages yet. Write one now; it stays here until another phone connects.")
                }
            }
            items(messages, key = { (entry, _) -> entry.id.toHex() }) { (entry, body) ->
                Card(Modifier.fillMaxWidth()) {
                    Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                        val author = if (entry.author.contentEquals(authorKey)) "You" else entry.author.toHex().take(16)
                        Text("$author · ${messageTime(entry.sortKeyMs)}", style = MaterialTheme.typography.labelSmall)
                        SelectionContainer { Text(body) }
                    }
                }
            }
        }
        OutlinedTextField(
            value = draft,
            onValueChange = onDraftChanged,
            modifier = Modifier.fillMaxWidth(),
            label = { Text("Public message") },
            maxLines = 4,
            enabled = !sending,
            supportingText = { Text("${draft.trim().toByteArray(Charsets.UTF_8).size} / ${BoardMessage.MAX_BYTES} bytes") },
        )
        Button(
            onClick = onSend,
            enabled = !sending && draft.isNotBlank() && draft.trim().toByteArray(Charsets.UTF_8).size <= BoardMessage.MAX_BYTES,
            modifier = Modifier.fillMaxWidth(),
        ) { Text(if (sending) "Saving…" else "Post to board") }
    }
}

private fun ByteArray.toHex(): String = joinToString("") { "%02x".format(it) }

private fun messageTime(ms: ULong): String =
    if (ms > Long.MAX_VALUE.toULong()) "Unknown time"
    else DateFormat.getDateTimeInstance(DateFormat.SHORT, DateFormat.SHORT).format(Date(ms.toLong()))

fun sizeLabel(bytes: Int): String = when {
    bytes >= 1_024 * 1_024 -> "${bytes / (1_024 * 1_024)} MiB"
    bytes >= 1_024 -> "${bytes / 1_024} KiB"
    else -> "$bytes B"
}
