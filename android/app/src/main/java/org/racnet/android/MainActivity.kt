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
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.createSavedStateHandle
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.material3.Text
import org.racnet.android.messages.BoardViewModel
import org.racnet.android.mesh.MeshService
import org.racnet.android.metrics.LinkMetricsSnapshot
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
        val app = RacnetApplication.from(this)
        val board = ViewModelProvider(this, viewModelFactory {
            initializer {
                BoardViewModel(createSavedStateHandle()) { kind, payload ->
                    withContext(Dispatchers.IO) { app.nodeRuntime.createEntry(kind, payload) }
                }
            }
        })[BoardViewModel::class.java]
        setContent {
            MaterialTheme {
                androidx.compose.material3.Surface {
                    App(app, board)
                }
            }
        }
    }
}

@Composable
private fun App(app: RacnetApplication, board: BoardViewModel) {
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
        Screen.Status -> StatusRoute(
            app,
            board = board,
            onShowDiagnostics = { screen = Screen.Diagnostics },
            onRequestPermissions = { screen = Screen.Onboarding },
        )
        Screen.Diagnostics -> DiagnosticsRoute(app, onBack = { screen = Screen.Status })
    }
}

@Composable
private fun StatusRoute(
    app: RacnetApplication,
    board: BoardViewModel,
    onShowDiagnostics: () -> Unit,
    onRequestPermissions: () -> Unit,
) {
    val running by MeshService.running.collectAsStateWithLifecycle()
    val peers by app.connectionRegistry.peers.collectAsStateWithLifecycle()
    val entryCount by app.nodeRuntime.entryCount.collectAsStateWithLifecycle()
    var entries by remember { mutableStateOf<List<EntryView>>(emptyList()) }
    var error by remember { mutableStateOf<String?>(null) }
    val boardState by board.state.collectAsStateWithLifecycle()
    val serviceError by MeshService.error.collectAsStateWithLifecycle()

    LaunchedEffect(entryCount) {
        entries = withContext(Dispatchers.IO) { app.nodeRuntime.entries() }
    }

    StatusScreen(
        running = running,
        fingerprintHex = app.nodeRuntime.fingerprint.joinToString("") { "%02x".format(it) },
        authorKey = app.nodeRuntime.authorKey,
        peers = peers,
        error = error ?: boardState.error ?: serviceError,
        sending = boardState.sending,
        entries = entries,
        draft = boardState.draft,
        onDraftChanged = board::updateDraft,
        onToggleService = { enable ->
            error = null
            if (enable && !Permissions.allGranted(app)) {
                onRequestPermissions()
            } else {
                try {
                    if (enable) MeshService.start(app) else MeshService.stop(app)
                } catch (e: Exception) {
                    error = "Cannot start mesh: ${e.message.orEmpty()}"
                }
            }
        },
        onCreateEntry = { size ->
            error = null
            board.createTestEntry(size)
        },
        onSend = {
            error = null
            board.post()
        },
        onShowDiagnostics = onShowDiagnostics,
    )
}

@Composable
private fun DiagnosticsRoute(app: RacnetApplication, onBack: () -> Unit) {
    var metrics by remember { mutableStateOf<List<LinkMetricsSnapshot>>(emptyList()) }
    LaunchedEffect(Unit) {
        metrics = app.connectionRegistry.connectionsSnapshot().map { it.metrics.snapshot() }
    }
    DiagnosticsScreen(
        metrics = metrics,
        onRefresh = {
            metrics = app.connectionRegistry.connectionsSnapshot().map { it.metrics.snapshot() }
        },
        onBack = onBack,
    )
}
