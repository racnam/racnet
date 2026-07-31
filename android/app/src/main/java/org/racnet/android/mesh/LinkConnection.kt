package org.racnet.android.mesh

import android.bluetooth.BluetoothSocket
import android.os.SystemClock
import java.io.IOException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.launch
import org.racnet.android.ble.BleConstants
import org.racnet.android.metrics.LinkMetrics
import org.racnet.android.metrics.Meas
import org.racnet.android.node.LinkTransport
import org.racnet.android.node.NodeRuntime
import uniffi.racnet_core.CloseCause

/**
 * One L2CAP CoC socket bound to one core link: a blocking read loop
 * feeding [NodeRuntime.onBytes], a write channel drained by a single
 * writer with frames chunked to the socket's outgoing MTU, and teardown
 * that reports [NodeRuntime.onTransportClosed] exactly once from
 * whichever side notices first.
 */
class LinkConnection(
    private val socket: BluetoothSocket,
    private val runtime: NodeRuntime,
    private val registry: ConnectionRegistry,
    private val parentScope: CoroutineScope,
    val initiator: Boolean,
    val metrics: LinkMetrics,
) : LinkTransport {

    val address: String = socket.remoteDevice?.address ?: "00:00:00:00:00:00"

    var linkId: ULong = ULong.MAX_VALUE
        private set

    // A child job: cancelling this connection's loops must never cancel
    // the parent (service) scope, only follow it.
    private val scope = CoroutineScope(
        parentScope.coroutineContext +
            SupervisorJob(parentScope.coroutineContext[Job]) +
            Dispatchers.IO,
    )
    private val writes = Channel<ByteArray>(Channel.UNLIMITED)

    /**
     * Registers the link with the core and starts the pump loops.
     * Returns false if the core refused it (rate limited): the socket is
     * closed silently and nothing was started.
     */
    suspend fun start(): Boolean {
        val open = if (initiator) {
            runtime.connect(this)
        } else {
            val addrBytes = BleConstants.addressToBytes(address) ?: ByteArray(6)
            runtime.accept(addrBytes, this)
        }
        if (open == null) {
            // §4.5: refused handshakes are dropped silently.
            closeSocketQuietly()
            return false
        }
        linkId = open.linkId
        metrics.l2capOpenAtMs = SystemClock.elapsedRealtime()
        registry.register(this)
        Meas.log(
            "link_open",
            "link" to linkId,
            "role" to if (initiator) "initiator" else "responder",
            "mtu" to maxTransmitSize(),
        )

        scope.launch { writeLoop() }
        scope.launch { readLoop() }
        return true
    }

    override fun writeFrames(frames: List<ByteArray>) {
        val mtu = maxTransmitSize()
        for (frame in frames) {
            metrics.bytesOut += frame.size
            for (chunk in FrameChunker.chunk(frame, mtu)) {
                // UNLIMITED channel: trySend only fails when closed, and
                // then the link is already dying.
                writes.trySend(chunk)
            }
        }
    }

    override fun closeFromCore(cause: CloseCause) {
        Meas.log("link_closed", "link" to linkId, "cause" to cause::class.simpleName.orEmpty())
        shutdown(reportToCore = false)
    }

    /**
     * Closes the socket and stops the loops. When the closure originates
     * here (socket death, duplicate-link teardown) the core is told; when
     * the core itself closed the link it must not be re-entered.
     */
    fun shutdown(reportToCore: Boolean = true) {
        writes.close()
        closeSocketQuietly()
        registry.remove(this)
        if (reportToCore) {
            parentScope.launch { runtime.onTransportClosed(linkId) }
        }
        scope.cancel()
    }

    private suspend fun readLoop() {
        val buffer = ByteArray(READ_BUFFER_SIZE)
        try {
            val input = socket.inputStream
            while (true) {
                val read = input.read(buffer)
                if (read < 0) break
                if (read == 0) continue
                metrics.bytesIn += read
                runtime.onBytes(linkId, buffer.copyOf(read))
            }
        } catch (e: IOException) {
            // Normal BLE link death; fall through to teardown.
        }
        shutdown(reportToCore = true)
    }

    private suspend fun writeLoop() {
        try {
            val output = socket.outputStream
            for (chunk in writes) {
                output.write(chunk)
            }
        } catch (e: IOException) {
            shutdown(reportToCore = true)
        }
    }

    private fun maxTransmitSize(): Int =
        try {
            socket.maxTransmitPacketSize
        } catch (e: Exception) {
            FrameChunker.MIN_MTU
        }

    private fun closeSocketQuietly() {
        try {
            socket.close()
        } catch (e: IOException) {
            // Already closed.
        }
    }

    private companion object {
        const val READ_BUFFER_SIZE = 65536
    }
}
