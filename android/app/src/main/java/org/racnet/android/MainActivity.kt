package org.racnet.android

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.launch
import org.racnet.android.mesh.MeshService
import org.racnet.android.metrics.LinkMetrics
import org.racnet.android.ui.DiagnosticsScreen
import org.racnet.android.ui.OnboardingScreen
import org.racnet.android.ui.Permissions
import org.racnet.android.ui.StatusScreen
import uniffi.racnet_core.EntryView

private enum class Screen { Onboarding, Status, Diagnostics }

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            MaterialTheme {
                App(RacnetApplication.from(this))
            }
        }
    }
}

@Composable
private fun App(app: RacnetApplication) {
    var screen by remember {
        mutableStateOf(
            if (Permissions.allGranted(app)) Screen.Status else Screen.Onboarding,
        )
    }
    when (screen) {
        Screen.Onboarding -> OnboardingScreen(onReady = { screen = Screen.Status })
        Screen.Status -> StatusRoute(app, onShowDiagnostics = { screen = Screen.Diagnostics })
        Screen.Diagnostics -> DiagnosticsRoute(app, onBack = { screen = Screen.Status })
    }
}

@Composable
private fun StatusRoute(app: RacnetApplication, onShowDiagnostics: () -> Unit) {
    val running by MeshService.running.collectAsStateWithLifecycle()
    val peers by app.connectionRegistry.peers.collectAsStateWithLifecycle()
    val entryCount by app.nodeRuntime.entryCount.collectAsStateWithLifecycle()
    var entries by remember { mutableStateOf<List<EntryView>>(emptyList()) }
    val scope = androidx.compose.runtime.rememberCoroutineScope()

    LaunchedEffect(entryCount) {
        entries = app.nodeRuntime.entries()
    }

    StatusScreen(
        running = running,
        fingerprintHex = app.nodeRuntime.fingerprint.joinToString("") { "%02x".format(it) },
        peers = peers,
        entries = entries,
        onToggleService = { enable ->
            if (enable) MeshService.start(app) else MeshService.stop(app)
        },
        onCreateEntry = { size ->
            scope.launch {
                // Distinguishable filler so cross-device inspection is easy.
                app.nodeRuntime.createEntry(0uL, ByteArray(size) { (it % 251).toByte() })
            }
        },
        onShowDiagnostics = onShowDiagnostics,
    )
}

@Composable
private fun DiagnosticsRoute(app: RacnetApplication, onBack: () -> Unit) {
    var metrics by remember { mutableStateOf<List<LinkMetrics>>(emptyList()) }
    LaunchedEffect(Unit) {
        metrics = app.connectionRegistry.connectionsSnapshot().map { it.metrics }
    }
    DiagnosticsScreen(
        metrics = metrics,
        onRefresh = {
            metrics = app.connectionRegistry.connectionsSnapshot().map { it.metrics }
        },
        onBack = onBack,
    )
}
