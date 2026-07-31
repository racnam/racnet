package org.racnet.android.ble

import android.annotation.SuppressLint
import android.bluetooth.BluetoothGatt
import android.bluetooth.BluetoothGattCharacteristic
import android.bluetooth.BluetoothGattServer
import android.bluetooth.BluetoothGattServerCallback
import android.bluetooth.BluetoothGattService
import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothServerSocket
import android.bluetooth.BluetoothSocket
import android.bluetooth.le.AdvertiseCallback
import android.bluetooth.le.AdvertiseData
import android.bluetooth.le.AdvertiseSettings
import android.bluetooth.le.BluetoothLeAdvertiser
import android.content.Context
import android.os.ParcelUuid
import android.os.SystemClock
import android.util.Log
import java.io.IOException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import org.racnet.android.mesh.ConnectionRegistry
import org.racnet.android.mesh.LinkConnection
import org.racnet.android.metrics.LinkMetrics
import org.racnet.android.metrics.Meas
import org.racnet.android.node.NodeRuntime

/**
 * The peripheral role of §9.1: listen on a dynamic L2CAP PSM, publish it
 * through the GATT characteristic, advertise the bare service UUID, and
 * hand accepted sockets to the core as responder links. Admission (the
 * §4.5 rate limiter) happens inside [NodeRuntime.accept]; a refused
 * socket is closed with nothing sent.
 *
 * Permissions are granted before the mesh service starts; radio calls
 * still guard against racing revocation.
 */
@SuppressLint("MissingPermission")
class BlePeripheral(
    private val context: Context,
    private val runtime: NodeRuntime,
    private val registry: ConnectionRegistry,
) {

    private var serverSocket: BluetoothServerSocket? = null
    private var gattServer: BluetoothGattServer? = null
    private var advertiser: BluetoothLeAdvertiser? = null

    private val advertiseCallback = object : AdvertiseCallback() {
        override fun onStartFailure(errorCode: Int) {
            Log.w(TAG, "advertising failed to start: $errorCode")
        }
    }

    /** Starts listening, the GATT server, and advertising. */
    fun start(scope: CoroutineScope): Boolean {
        val manager = context.getSystemService(BluetoothManager::class.java)
        val adapter = manager?.adapter ?: return false
        if (!adapter.isEnabled) return false

        val socket = try {
            adapter.listenUsingInsecureL2capChannel()
        } catch (e: IOException) {
            Log.w(TAG, "L2CAP listen failed", e)
            return false
        } catch (e: SecurityException) {
            Log.w(TAG, "missing Bluetooth permission", e)
            return false
        }
        serverSocket = socket
        val psm = socket.psm
        Meas.log("listening", "psm" to psm)

        if (!startGattServer(manager, psm)) {
            stop()
            return false
        }
        startAdvertising(adapter.bluetoothLeAdvertiser)

        scope.launch(Dispatchers.IO) { acceptLoop(scope, socket) }
        return true
    }

    fun stop() {
        try {
            serverSocket?.close()
        } catch (e: IOException) {
            // Already closed.
        }
        serverSocket = null
        try {
            advertiser?.stopAdvertising(advertiseCallback)
        } catch (e: SecurityException) {
            // Permission revoked mid-flight; nothing left to stop.
        }
        advertiser = null
        try {
            gattServer?.close()
        } catch (e: SecurityException) {
            // As above.
        }
        gattServer = null
    }

    private fun startGattServer(manager: BluetoothManager, psm: Int): Boolean {
        val psmBytes = BleConstants.psmToBytes(psm)
        val callback = object : BluetoothGattServerCallback() {
            override fun onCharacteristicReadRequest(
                device: android.bluetooth.BluetoothDevice,
                requestId: Int,
                offset: Int,
                characteristic: BluetoothGattCharacteristic,
            ) {
                val value = if (characteristic.uuid == BleConstants.PSM_CHARACTERISTIC_UUID &&
                    offset <= psmBytes.size
                ) {
                    psmBytes.copyOfRange(offset, psmBytes.size)
                } else {
                    null
                }
                try {
                    gattServer?.sendResponse(
                        device,
                        requestId,
                        if (value != null) BluetoothGatt.GATT_SUCCESS else BluetoothGatt.GATT_FAILURE,
                        offset,
                        value,
                    )
                } catch (e: SecurityException) {
                    // Permission revoked mid-flight.
                }
            }
        }
        val server = try {
            manager.openGattServer(context, callback)
        } catch (e: SecurityException) {
            Log.w(TAG, "missing Bluetooth permission for GATT server", e)
            null
        } ?: return false
        gattServer = server

        val service = BluetoothGattService(
            BleConstants.SERVICE_UUID,
            BluetoothGattService.SERVICE_TYPE_PRIMARY,
        )
        service.addCharacteristic(
            BluetoothGattCharacteristic(
                BleConstants.PSM_CHARACTERISTIC_UUID,
                BluetoothGattCharacteristic.PROPERTY_READ,
                BluetoothGattCharacteristic.PERMISSION_READ,
            ),
        )
        return server.addService(service)
    }

    private fun startAdvertising(leAdvertiser: BluetoothLeAdvertiser?) {
        advertiser = leAdvertiser ?: return
        val settings = AdvertiseSettings.Builder()
            .setAdvertiseMode(AdvertiseSettings.ADVERTISE_MODE_BALANCED)
            .setTxPowerLevel(AdvertiseSettings.ADVERTISE_TX_POWER_HIGH)
            .setConnectable(true)
            .build()
        // §9.1.2: the service UUID is the entire advertisement.
        val data = AdvertiseData.Builder()
            .addServiceUuid(ParcelUuid(BleConstants.SERVICE_UUID))
            .setIncludeDeviceName(false)
            .setIncludeTxPowerLevel(false)
            .build()
        try {
            leAdvertiser.startAdvertising(settings, data, advertiseCallback)
        } catch (e: SecurityException) {
            Log.w(TAG, "missing advertise permission", e)
        }
    }

    private suspend fun acceptLoop(scope: CoroutineScope, socket: BluetoothServerSocket) {
        while (scope.isActive) {
            val accepted: BluetoothSocket = try {
                socket.accept()
            } catch (e: IOException) {
                // Server socket closed: normal stop path.
                return
            }
            val metrics = LinkMetrics(
                address = accepted.remoteDevice?.address ?: "unknown",
                initiator = false,
            )
            metrics.l2capOpenAtMs = SystemClock.elapsedRealtime()
            LinkConnection(
                socket = accepted,
                runtime = runtime,
                registry = registry,
                parentScope = scope,
                initiator = false,
                metrics = metrics,
            ).start()
        }
    }

    private companion object {
        const val TAG = "RacnetPeripheral"
    }
}
