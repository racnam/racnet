package org.racnet.android

import android.app.Application
import android.app.NotificationChannel
import android.app.NotificationManager
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import org.racnet.android.identity.IdentityStore
import org.racnet.android.mesh.ConnectionRegistry
import org.racnet.android.node.NodeRuntime

/**
 * Application entry point: creates the notification channel and holds the
 * process-wide singletons (manual service locator; ADR-0016 rejects DI
 * frameworks at this size).
 */
class RacnetApplication : Application() {

    lateinit var identityStore: IdentityStore
        private set

    lateinit var nodeRuntime: NodeRuntime
        private set

    lateinit var connectionRegistry: ConnectionRegistry
        private set

    private val appScope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val _ready = MutableStateFlow(false)
    val ready: StateFlow<Boolean> = _ready
    private val _startupError = MutableStateFlow<String?>(null)
    val startupError: StateFlow<String?> = _startupError

    override fun onCreate() {
        super.onCreate()
        identityStore = IdentityStore(this)
        appScope.launch {
            try {
                nodeRuntime = NodeRuntime(
                    identityStore.loadOrCreate(),
                    java.io.File(filesDir, "entries.racnet").absolutePath,
                )
                connectionRegistry = ConnectionRegistry(nodeRuntime.fingerprint)
                _ready.value = true
            } catch (e: Exception) {
                _startupError.value = "Cannot open local data. Your files have been preserved. " +
                    "Free storage if needed, then reopen Racnet. ${e.message.orEmpty()}"
            }
        }

        val manager = getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(
            NotificationChannel(
                MESH_CHANNEL_ID,
                "Mesh service",
                // Low importance: persistent status, never a sound.
                NotificationManager.IMPORTANCE_LOW,
            ),
        )
    }

    companion object {
        const val MESH_CHANNEL_ID = "mesh"

        fun from(context: android.content.Context): RacnetApplication =
            context.applicationContext as RacnetApplication
    }
}
