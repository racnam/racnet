package org.racnet.android.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Card
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import org.racnet.android.metrics.LinkMetrics
import org.racnet.android.metrics.Meas

/**
 * Per-link phase timings, byte counters, and computed throughput —
 * copyable, so hardware runs transcribe into `docs/MEASUREMENTS.md` via
 * the procedures in `docs/MEASUREMENT-PROCEDURES.md`.
 */
@Composable
fun DiagnosticsScreen(
    metrics: List<LinkMetrics>,
    onRefresh: () -> Unit,
    onBack: () -> Unit,
) {
    val clipboard = LocalClipboardManager.current
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        TextButton(onClick = onBack) { Text("← Back") }
        Text("Diagnostics", style = MaterialTheme.typography.headlineMedium)
        Text(
            "Capture during runs: adb logcat -s ${Meas.TAG}",
            fontFamily = FontFamily.Monospace,
            style = MaterialTheme.typography.bodySmall,
        )
        OutlinedButton(onClick = onRefresh) { Text("Refresh") }
        OutlinedButton(onClick = {
            clipboard.setText(AnnotatedString(metrics.joinToString("\n") { report(it) }))
        }) { Text("Copy all") }

        if (metrics.isEmpty()) {
            Text("No live links.", style = MaterialTheme.typography.bodyMedium)
        }
        LazyColumn(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            items(metrics) { linkMetrics ->
                Card(modifier = Modifier.fillMaxWidth()) {
                    Text(
                        report(linkMetrics),
                        fontFamily = FontFamily.Monospace,
                        style = MaterialTheme.typography.bodySmall,
                        modifier = Modifier.padding(12.dp),
                    )
                }
            }
        }
    }
}

/** One link's diagnostics as stable, transcribable text. */
fun report(metrics: LinkMetrics): String = buildString {
    append(metrics.address)
    append(if (metrics.initiator) " (dialed)" else " (accepted)")
    for ((label, ms) in metrics.phases()) {
        append("\n  ").append(label).append(": ").append(ms).append(" ms")
    }
    append("\n  bytes in/out: ").append(metrics.bytesIn).append('/').append(metrics.bytesOut)
    val syncWindow = metrics.syncDoneAtMs - metrics.establishedAtMs
    if (metrics.syncDoneAtMs > 0 && syncWindow > 0) {
        append("\n  sync throughput in: ")
            .append(Meas.kbps(metrics.bytesIn, syncWindow))
            .append(" kbit/s")
    }
}
