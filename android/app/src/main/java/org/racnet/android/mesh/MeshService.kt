package org.racnet.android.mesh

import android.app.Notification
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.BroadcastReceiver
import android.content.IntentFilter
import android.bluetooth.BluetoothAdapter
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.CoroutineStart
import android.content.pm.ServiceInfo
import android.os.SystemClock
import androidx.core.app.NotificationCompat
import androidx.lifecycle.LifecycleService
import androidx.lifecycle.lifecycleScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.launch
import org.racnet.android.MainActivity
import org.racnet.android.R
import org.racnet.android.RacnetApplication
import org.racnet.android.ble.BleCentral
import org.racnet.android.ble.BlePeripheral
import org.racnet.android.metrics.Meas
import uniffi.racnet_core.Event

/**
 * The foreground service keeping the mesh alive: dual-role BLE (both
 * roles at once, §9.1.1), the core tick loop, event wiring into the
 * measurement log, and a persistent notification whose
 * content is the mesh status (ADR-0016).
 */
class MeshService : LifecycleService() {

    private lateinit var app: RacnetApplication
    private lateinit var peripheral: BlePeripheral
    private lateinit var central: BleCentral

    private var starting = false
    private val radioReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            if (intent.getIntExtra(BluetoothAdapter.EXTRA_STATE, -1) == BluetoothAdapter.STATE_OFF) {
                fail("Bluetooth is off. Enable it, then turn the mesh on again.")
            }
        }
    }

    override fun onCreate() {
        super.onCreate()
        app = RacnetApplication.from(this)
        androidx.core.content.ContextCompat.registerReceiver(
            this, radioReceiver, IntentFilter(BluetoothAdapter.ACTION_STATE_CHANGED),
            androidx.core.content.ContextCompat.RECEIVER_EXPORTED,
        )
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        super.onStartCommand(intent, flags, startId)
        try {
            startForeground(
                NOTIFICATION_ID, buildNotification(peers = 0, entries = 0uL),
                ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE,
            )
        } catch (e: SecurityException) {
            fail("Bluetooth permission is missing. Reopen Racnet to grant it.")
            return START_NOT_STICKY
        }
        if (!starting) {
            starting = true
            errorState.value = null
            lifecycleScope.launch {
                combine(app.ready, app.startupError) { ready, error -> ready || error != null }.first { it }
                if (!app.ready.value) {
                    fail(app.startupError.value ?: "Local data is unavailable.")
                    return@launch
                }
                app.nodeRuntime.clearReceiveFailure()
                peripheral = BlePeripheral(this@MeshService, app.nodeRuntime, app.connectionRegistry, ::fail)
                central = BleCentral(this@MeshService, app.nodeRuntime, app.connectionRegistry, onFailure = ::fail)
                app.nodeRuntime.startTicking(lifecycleScope)
                wireReceiveFailure()
                wireEvents()
                wireNotification()
                try {
                    if (!peripheral.start(lifecycleScope) || !central.start(lifecycleScope)) {
                        fail("Could not start Bluetooth mesh. Check Bluetooth and permissions, then try again.")
                        return@launch
                    }
                    runningState.value = true
                    Meas.log("service_started", "t_ms" to SystemClock.elapsedRealtime())
                } catch (e: SecurityException) {
                    fail("Bluetooth permission was revoked. Reopen Racnet to grant it.")
                }
            }
        }
        return START_STICKY
    }

    override fun onDestroy() {
        runningState.value = false
        unregisterReceiver(radioReceiver)
        if (::central.isInitialized) central.stop()
        if (::peripheral.isInitialized) peripheral.stop()
        if (app.ready.value) app.connectionRegistry.shutdownAll()
        Meas.log("service_stopped", "t_ms" to SystemClock.elapsedRealtime())
        super.onDestroy()
    }

    private fun fail(message: String) {
        errorState.value = message
        runningState.value = false
        stopSelf()
    }

    private fun wireReceiveFailure() {
        lifecycleScope.launch(start = CoroutineStart.UNDISPATCHED) {
            app.nodeRuntime.receiveFailure.collect { failure ->
                if (failure != null) {
                    errorState.value = "Local storage or sync resources are full. Some messages were not received."
                }
            }
        }
    }

    /** Observes core events for diagnostics; lifecycle routing is synchronous. */
    private fun wireEvents() {
        val registry = app.connectionRegistry
        lifecycleScope.launch(start = CoroutineStart.UNDISPATCHED) {
            app.nodeRuntime.events.collect { event ->
                when (event) {
                    is Event.Reconciled -> {
                        Meas.log(
                            "reconciled",
                            "link" to event.linkId,
                            "sid" to event.sessionId,
                            "have" to event.have.size,
                            "need" to event.need.size,
                        )
                    }
                    is Event.SyncSessionClosed -> {
                        val metrics = registry.connection(event.linkId)?.metrics ?: return@collect
                        metrics.syncDoneAtMs = SystemClock.elapsedRealtime()
                        val duration = metrics.syncDoneAtMs - metrics.l2capOpenAtMs
                        Meas.log(
                            "sync_done",
                            "link" to event.linkId,
                            "sid" to event.sessionId,
                            "bytes_in" to metrics.bytesIn,
                            "bytes_out" to metrics.bytesOut,
                            "link_dur_ms" to duration,
                            "entries" to app.nodeRuntime.entryCount.value,
                            "link_avg_in_kbps" to Meas.kbps(metrics.bytesIn, duration),
                        )
                    }
                    else -> {}
                }
            }
        }
    }

    private fun wireNotification() {
        lifecycleScope.launch {
            combine(
                app.connectionRegistry.peers,
                app.nodeRuntime.entryCount,
            ) { peers, entries -> Pair(peers.size, entries) }
                .collect { (peers, entries) ->
                    val manager =
                        getSystemService(android.app.NotificationManager::class.java)
                    manager.notify(NOTIFICATION_ID, buildNotification(peers, entries))
                }
        }
    }

    private fun buildNotification(peers: Int, entries: ULong): Notification {
        val tapIntent = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE,
        )
        return NotificationCompat.Builder(this, RacnetApplication.MESH_CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle("Racnet mesh")
            .setContentText("$peers peers · $entries entries")
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setContentIntent(tapIntent)
            .build()
    }

    companion object {
        private const val NOTIFICATION_ID = 1

        private val errorState = MutableStateFlow<String?>(null)
        val error: StateFlow<String?> = errorState

        private val runningState = MutableStateFlow(false)

        /** Whether the service is running, for the UI toggle. */
        val running: StateFlow<Boolean> = runningState

        fun start(context: Context) {
            context.startForegroundService(Intent(context, MeshService::class.java))
        }

        fun stop(context: Context) {
            context.stopService(Intent(context, MeshService::class.java))
        }
    }
}
