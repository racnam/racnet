package org.racnet.android.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Card
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import org.racnet.android.mesh.PeerInfo
import uniffi.racnet_core.EntryView

/** The test-entry payload sizes procedure P1 measures with. */
val TEST_ENTRY_SIZES = listOf(100, 1_024, 10_240, 102_400)

/**
 * Mesh status: the service toggle, our fingerprint, established peers,
 * stored entries, and the create-test-entry buttons whose payload sizes
 * back the throughput procedure.
 */
@Composable
fun StatusScreen(
    running: Boolean,
    fingerprintHex: String,
    peers: List<PeerInfo>,
    entries: List<EntryView>,
    onToggleService: (Boolean) -> Unit,
    onCreateEntry: (Int) -> Unit,
    onShowDiagnostics: () -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Text("Racnet", style = MaterialTheme.typography.headlineMedium)
            Switch(checked = running, onCheckedChange = onToggleService)
        }
        Text(
            "You: ${fingerprintHex.take(16)}…",
            fontFamily = FontFamily.Monospace,
            style = MaterialTheme.typography.bodySmall,
        )
        TextButton(onClick = onShowDiagnostics) { Text("Diagnostics") }

        Card(modifier = Modifier.fillMaxWidth()) {
            Column(modifier = Modifier.padding(12.dp)) {
                Text(
                    "Peers (${peers.size})",
                    style = MaterialTheme.typography.titleMedium,
                )
                if (peers.isEmpty()) {
                    Text(
                        if (running) "Searching nearby…" else "Mesh is off",
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
                peers.forEach { peer ->
                    Text(
                        "${peer.fingerprintHex.take(16)}… " +
                            if (peer.initiator) "(dialed)" else "(accepted)",
                        fontFamily = FontFamily.Monospace,
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
            }
        }

        Card(modifier = Modifier.fillMaxWidth()) {
            Column(modifier = Modifier.padding(12.dp)) {
                Text("Create test entry", style = MaterialTheme.typography.titleMedium)
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    TEST_ENTRY_SIZES.forEach { size ->
                        OutlinedButton(onClick = { onCreateEntry(size) }) {
                            Text(sizeLabel(size))
                        }
                    }
                }
            }
        }

        Text(
            "Entries (${entries.size})",
            style = MaterialTheme.typography.titleMedium,
        )
        LazyColumn(modifier = Modifier.fillMaxWidth()) {
            items(entries.asReversed()) { entry ->
                Column(modifier = Modifier.padding(vertical = 4.dp)) {
                    Text(
                        entry.id.joinToString("") { "%02x".format(it) }.take(16) +
                            "… · kind ${entry.kind} · ${entry.payload.size} B",
                        fontFamily = FontFamily.Monospace,
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
                HorizontalDivider()
            }
        }
    }
}

fun sizeLabel(bytes: Int): String = when {
    bytes >= 1_024 * 1_024 -> "${bytes / (1_024 * 1_024)} MiB"
    bytes >= 1_024 -> "${bytes / 1_024} KiB"
    else -> "$bytes B"
}
