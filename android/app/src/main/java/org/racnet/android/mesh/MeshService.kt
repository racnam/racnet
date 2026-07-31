package org.racnet.android.mesh

import android.app.Notification
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
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
 * registry and the measurement log, and a persistent notification whose
 * content is the mesh status (ADR-0016).
 */
class MeshService : LifecycleService() {

    private lateinit var app: RacnetApplication
    private lateinit var peripheral: BlePeripheral
    private lateinit var central: BleCentral

    override fun onCreate() {
        super.onCreate()
        app = RacnetApplication.from(this)
        peripheral = BlePeripheral(this, app.nodeRuntime, app.connectionRegistry)
        central = BleCentral(this, app.nodeRuntime, app.connectionRegistry)
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        super.onStartCommand(intent, flags, startId)
        startForeground(
            NOTIFICATION_ID,
            buildNotification(peers = 0, entries = 0uL),
            ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE,
        )
        if (!runningState.value) {
            runningState.value = true
            app.nodeRuntime.startTicking(lifecycleScope)
            wireEvents()
            wireNotification()
            peripheral.start(lifecycleScope)
            central.start(lifecycleScope)
            Meas.log("service_started", "t_ms" to SystemClock.elapsedRealtime())
        }
        return START_STICKY
    }

    override fun onDestroy() {
        runningState.value = false
        central.stop()
        peripheral.stop()
        app.connectionRegistry.shutdownAll()
        Meas.log("service_stopped", "t_ms" to SystemClock.elapsedRealtime())
        super.onDestroy()
    }

    /** Routes core events to the registry and the measurement log. */
    private fun wireEvents() {
        val registry = app.connectionRegistry
        lifecycleScope.launch {
            app.nodeRuntime.events.collect { event ->
                when (event) {
                    is Event.Established ->
                        registry.onEstablished(event.linkId, event.remoteFingerprint)
                    is Event.Reconciled -> {
                        registry.connection(event.linkId)?.metrics?.syncOpenedAtMs =
                            SystemClock.elapsedRealtime()
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
                        val duration = metrics.syncDoneAtMs - metrics.establishedAtMs
                        Meas.log(
                            "sync_done",
                            "link" to event.linkId,
                            "sid" to event.sessionId,
                            "bytes_in" to metrics.bytesIn,
                            "bytes_out" to metrics.bytesOut,
                            "dur_ms" to duration,
                            "tput_in_kbps" to Meas.kbps(metrics.bytesIn, duration),
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
