package org.racnet.android

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.launch
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.coroutines.CancellationException
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.material3.Text
import org.racnet.android.messages.BoardMessage
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
        androidx.core.view.WindowCompat.getInsetsController(window, window.decorView).apply {
            isAppearanceLightStatusBars = true
            isAppearanceLightNavigationBars = true
        }
        setContent {
            MaterialTheme {
                androidx.compose.material3.Surface {
                    App(RacnetApplication.from(this))
                }
            }
        }
    }
}

@Composable
private fun App(app: RacnetApplication) {
    val ready by app.ready.collectAsStateWithLifecycle()
    val startupError by app.startupError.collectAsStateWithLifecycle()
    if (!ready) {
        Text(startupError ?: "Opening local messages…", Modifier.safeDrawingPadding().padding(24.dp))
        return
    }
    var screen by rememberSaveable {
        mutableStateOf(
            if (Permissions.allGranted(app)) Screen.Status else Screen.Onboarding,
        )
    }
    BackHandler(enabled = screen == Screen.Diagnostics) { screen = Screen.Status }
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
    var error by remember { mutableStateOf<String?>(null) }
    var sending by remember { mutableStateOf(false) }
    val serviceError by MeshService.error.collectAsStateWithLifecycle()

    LaunchedEffect(entryCount) {
        entries = withContext(Dispatchers.IO) { app.nodeRuntime.entries() }
    }

    StatusScreen(
        running = running,
        fingerprintHex = app.nodeRuntime.fingerprint.joinToString("") { "%02x".format(it) },
        authorKey = app.nodeRuntime.authorKey,
        peers = peers,
        error = error ?: serviceError,
        sending = sending,
        entries = entries,
        onToggleService = { enable ->
            error = null
            if (enable && !Permissions.allGranted(app)) {
                error = "Bluetooth permissions are missing. Reopen Racnet to grant them."
            } else {
                try {
                    if (enable) MeshService.start(app) else MeshService.stop(app)
                } catch (e: Exception) {
                    error = "Cannot start mesh: ${e.message.orEmpty()}"
                }
            }
        },
        onCreateEntry = { size ->
            scope.launch {
                sending = true
                try {
                    withContext(Dispatchers.IO) {
                        app.nodeRuntime.createEntry(0uL, ByteArray(size) { (it % 251).toByte() })
                    }
                    error = null
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    error = "Entry was not saved: ${e.message.orEmpty()}"
                } finally { sending = false }
            }
        },
        onSend = { text, saved ->
            scope.launch {
                sending = true
                try {
                    val payload = BoardMessage.encode(text)
                    withContext(Dispatchers.IO) {
                        app.nodeRuntime.createEntry(BoardMessage.KIND, payload)
                    }
                    error = null
                    saved()
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    error = "Message was not saved: ${e.message.orEmpty()}"
                } finally { sending = false }
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
