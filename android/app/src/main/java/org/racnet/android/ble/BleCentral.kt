package org.racnet.android.ble

import android.annotation.SuppressLint
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothGatt
import android.bluetooth.BluetoothGattCallback
import android.bluetooth.BluetoothGattCharacteristic
import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothProfile
import android.bluetooth.le.BluetoothLeScanner
import android.bluetooth.le.ScanCallback
import android.bluetooth.le.ScanFilter
import android.bluetooth.le.ScanResult
import android.bluetooth.le.ScanSettings
import android.content.Context
import android.os.ParcelUuid
import android.os.SystemClock
import android.util.Log
import java.io.IOException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import org.racnet.android.mesh.ConnectionRegistry
import org.racnet.android.mesh.LinkConnection
import org.racnet.android.metrics.LinkMetrics
import org.racnet.android.metrics.Meas
import org.racnet.android.node.NodeRuntime
import org.racnet.android.policy.ConnectionPolicy

/**
 * The central role of §9.1: one long-lived scan filtered on the service
 * UUID, the [ConnectionPolicy]'s jitter and backoff per address, a GATT
 * read of the PSM characteristic (connection closed before the channel
 * opens, §9.1.3), then the L2CAP connect as the link initiator.
 *
 * The policy object is confined to a single-parallelism dispatcher; scan
 * callbacks arrive on binder threads and are bounced there.
 */
@SuppressLint("MissingPermission")
class BleCentral(
    private val context: Context,
    private val runtime: NodeRuntime,
    private val registry: ConnectionRegistry,
    private val policy: ConnectionPolicy = ConnectionPolicy(),
) {

    private var scanner: BluetoothLeScanner? = null
    private var scope: CoroutineScope? = null

    @OptIn(kotlinx.coroutines.ExperimentalCoroutinesApi::class)
    private val policyDispatcher = Dispatchers.Default.limitedParallelism(1)

    private val scanCallback = object : ScanCallback() {
        override fun onScanResult(callbackType: Int, result: ScanResult) {
            val active = scope ?: return
            active.launch(policyDispatcher) { handleScanResult(active, result) }
        }

        override fun onScanFailed(errorCode: Int) {
            Log.w(TAG, "scan failed: $errorCode")
        }
    }

    fun start(scope: CoroutineScope): Boolean {
        val adapter = context.getSystemService(BluetoothManager::class.java)?.adapter
            ?: return false
        if (!adapter.isEnabled) return false
        val leScanner = adapter.bluetoothLeScanner ?: return false
        this.scope = scope
        this.scanner = leScanner

        val filter = ScanFilter.Builder()
            .setServiceUuid(ParcelUuid(BleConstants.SERVICE_UUID))
            .build()
        val settings = ScanSettings.Builder()
            .setScanMode(ScanSettings.SCAN_MODE_BALANCED)
            .setCallbackType(ScanSettings.CALLBACK_TYPE_ALL_MATCHES)
            .build()
        return try {
            leScanner.startScan(listOf(filter), settings, scanCallback)
            true
        } catch (e: SecurityException) {
            Log.w(TAG, "missing scan permission", e)
            false
        }
    }

    fun stop() {
        try {
            scanner?.stopScan(scanCallback)
        } catch (e: SecurityException) {
            // Permission revoked mid-flight; the scan is dead either way.
        }
        scanner = null
        scope = null
    }

    private suspend fun handleScanResult(scope: CoroutineScope, result: ScanResult) {
        val device = result.device ?: return
        val address = device.address ?: return
        if (registry.hasAddress(address)) return
        val jitterMs = policy.shouldConnect(address, SystemClock.elapsedRealtime()) ?: return

        val metrics = LinkMetrics(address = address, initiator = true)
        metrics.scanFoundAtMs = SystemClock.elapsedRealtime()
        scope.launch(policyDispatcher) {
            // §9.1.5: uniform random delay before initiating, so crossed
            // simultaneous connections stay rare.
            delay(jitterMs)
            connectTo(scope, device, metrics)
        }
    }

    private suspend fun connectTo(
        scope: CoroutineScope,
        device: BluetoothDevice,
        metrics: LinkMetrics,
    ) {
        val address = device.address
        val psm = readPsm(device, metrics)
        if (psm == null) {
            policy.failed(address, SystemClock.elapsedRealtime())
            return
        }
        Meas.log(
            "psm_read",
            "addr" to address,
            "psm" to psm,
            "scan_to_psm_ms" to (metrics.psmReadAtMs - metrics.scanFoundAtMs),
        )

        val connection = openL2cap(scope, device, psm, metrics)
        if (connection == null) {
            policy.failed(address, SystemClock.elapsedRealtime())
            return
        }
        policy.established(address)
    }

    private suspend fun openL2cap(
        scope: CoroutineScope,
        device: BluetoothDevice,
        psm: Int,
        metrics: LinkMetrics,
    ): LinkConnection? = kotlinx.coroutines.withContext(Dispatchers.IO) {
        val address = device.address
        val socket = try {
            device.createInsecureL2capChannel(psm)
        } catch (e: IOException) {
            return@withContext null
        } catch (e: SecurityException) {
            return@withContext null
        }
        try {
            // Blocking; bounded by the stack's own L2CAP connect timeout.
            socket.connect()
        } catch (e: IOException) {
            Log.i(TAG, "L2CAP connect to $address failed")
            try {
                socket.close()
            } catch (closeError: IOException) {
                // Already gone.
            }
            return@withContext null
        }
        metrics.l2capOpenAtMs = SystemClock.elapsedRealtime()
        val connection = LinkConnection(
            socket = socket,
            runtime = runtime,
            registry = registry,
            parentScope = scope,
            initiator = true,
            metrics = metrics,
            // Return the address to the policy however the link dies —
            // the callback fires exactly once, from whichever side
            // notices first, with no subscription race.
            onTeardown = {
                scope.launch(policyDispatcher) { policy.closed(address) }
            },
        )
        if (connection.start()) connection else null
    }

    /**
     * GATT phase: connect, discover, read the PSM characteristic, close.
     * Returns null on any failure or timeout.
     */
    private suspend fun readPsm(device: BluetoothDevice, metrics: LinkMetrics): Int? {
        val result = kotlinx.coroutines.CompletableDeferred<Int?>()
        var gatt: BluetoothGatt? = null
        val callback = object : BluetoothGattCallback() {
            override fun onConnectionStateChange(g: BluetoothGatt, status: Int, newState: Int) {
                when {
                    status != BluetoothGatt.GATT_SUCCESS -> result.complete(null)
                    newState == BluetoothProfile.STATE_CONNECTED -> {
                        metrics.gattConnectedAtMs = SystemClock.elapsedRealtime()
                        if (!g.discoverServices()) result.complete(null)
                    }
                    newState == BluetoothProfile.STATE_DISCONNECTED -> result.complete(null)
                }
            }

            override fun onServicesDiscovered(g: BluetoothGatt, status: Int) {
                val characteristic = g
                    .getService(BleConstants.SERVICE_UUID)
                    ?.getCharacteristic(BleConstants.PSM_CHARACTERISTIC_UUID)
                if (status != BluetoothGatt.GATT_SUCCESS ||
                    characteristic == null ||
                    !g.readCharacteristic(characteristic)
                ) {
                    result.complete(null)
                }
            }

            @Deprecated("pre-33 read callback; still delivered on all supported APIs")
            override fun onCharacteristicRead(
                g: BluetoothGatt,
                characteristic: BluetoothGattCharacteristic,
                status: Int,
            ) {
                if (status == BluetoothGatt.GATT_SUCCESS) {
                    metrics.psmReadAtMs = SystemClock.elapsedRealtime()
                    @Suppress("DEPRECATION")
                    result.complete(BleConstants.psmFromBytes(characteristic.value))
                } else {
                    result.complete(null)
                }
            }
        }
        return try {
            gatt = device.connectGatt(context, false, callback, BluetoothDevice.TRANSPORT_LE)
            withTimeoutOrNull(GATT_TIMEOUT_MS) { result.await() }
        } catch (e: SecurityException) {
            null
        } finally {
            // §9.1.3: the GATT connection closes before the channel opens.
            try {
                gatt?.close()
            } catch (e: SecurityException) {
                // Nothing left to release.
            }
        }
    }

    private companion object {
        const val TAG = "RacnetCentral"
        const val GATT_TIMEOUT_MS = 10_000L
    }
}
