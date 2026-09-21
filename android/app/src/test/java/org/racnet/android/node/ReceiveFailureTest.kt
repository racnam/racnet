package org.racnet.android.node

import java.io.File
import java.nio.file.Files
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.*
import org.junit.Test
import uniffi.racnet_core.CloseCause
import uniffi.racnet_core.Identity
import uniffi.racnet_core.Node
import uniffi.racnet_core.SyncWindow

class ReceiveFailureTest {
    private class Pipe : LinkTransport {
        val frames = ArrayDeque<ByteArray>()
        var established = false
        var closeCause: CloseCause? = null
        var drainOnClose = false
        override fun writeFrames(frames: List<ByteArray>) { this.frames.addAll(frames) }
        override fun onEstablished(remoteFingerprint: ByteArray) { established = true }
        override fun closeFromCore(cause: CloseCause, drainWrites: Boolean) {
            closeCause = cause
            drainOnClose = drainWrites
        }
    }

    private fun identity(seed: Int) =
        Identity(ByteArray(32) { seed.toByte() }, ByteArray(32) { (seed + 32).toByte() })

    private suspend fun ordinaryClose(runtime: NodeRuntime) {
        runtime.onTransportClosed(runtime.connect(Pipe()).linkId)
    }

    /** A real peer exceeds the receiver's concurrent-session budget. */
    private suspend fun exhaustReceiveSessions(runtime: NodeRuntime) {
        val peer = Node(identity(21))
        val outgoing = peer.connect(0uL)
        val pending = ArrayDeque(outgoing.initialFrames)
        val transport = Pipe()
        val incoming = runtime.accept(byteArrayOf(1, 2, 3, 4, 5, 6), transport)!!
        try {
            repeat(20) {
                if (!transport.established) {
                    while (pending.isNotEmpty() && !transport.established) {
                        runtime.onBytes(incoming.linkId, pending.removeFirst())
                    }
                    if (!transport.established) {
                        while (transport.frames.isNotEmpty()) {
                            pending.addAll(peer.onBytes(outgoing.linkId, transport.frames.removeFirst(), 0uL).frames)
                        }
                    }
                }
            }
            assertTrue("handshake completed", transport.established)

            // Hold the runtime's initial RECON_INIT and all replies. It has
            // one locally opened session, while the raw peer has none yet.
            // Eight peer opens fit there but exceed the receiver's budget.
            repeat(8) {
                val opened = peer.startSync(outgoing.linkId, SyncWindow(0uL, ULong.MAX_VALUE))
                opened.io.frames.forEach { runtime.onBytes(incoming.linkId, it) }
            }
            assertEquals(4uL, (transport.closeCause as? CloseCause.ProtocolViolation)?.code)
            assertTrue("terminal ERROR must be drained", transport.drainOnClose)
        } finally {
            runtime.onTransportClosed(incoming.linkId)
            peer.close()
        }
    }

    @Test fun receiveWarningIsRetainedWithoutDiagnosticCollectors() = verifyWarning(false)

    @Test fun receiveWarningSurvivesOverflowingDiagnosticCollectors() = verifyWarning(true)

    @Test fun silentCloseMustDiscardPreviouslyQueuedFrames() = runTest {
        val dir = Files.createTempDirectory("racnet-silent-close").toFile()
        val runtime = NodeRuntime(identity(22), File(dir, "entries").path) { 0uL }
        try {
            val transport = Pipe()
            val open = runtime.connect(transport)
            assertTrue("initial HELLO is still queued", transport.frames.isNotEmpty())
            runtime.onBytes(open.linkId, byteArrayOf(0, 0))
            assertTrue(transport.closeCause is CloseCause.HandshakeFailed)
            assertFalse("silent failure must not flush the old HELLO", transport.drainOnClose)
        } finally {
            runtime.close()
            dir.deleteRecursively()
        }
    }

    @OptIn(kotlinx.coroutines.ExperimentalCoroutinesApi::class)
    private fun verifyWarning(stallCollector: Boolean) = runTest {
        val dir = Files.createTempDirectory("racnet-receive-failure").toFile()
        val runtime = NodeRuntime(identity(20), File(dir, "entries").path) { 0uL }
        var observed = 0
        val observer = if (stallCollector) {
            backgroundScope.launch(start = CoroutineStart.UNDISPATCHED) {
                runtime.events.collect {
                    observed++
                    awaitCancellation()
                }
            }
        } else null
        try {
            assertNull(runtime.receiveFailure.value)
            if (stallCollector) {
                ordinaryClose(runtime)
                runCurrent()
                assertEquals(1, observed)
                repeat(1100) { ordinaryClose(runtime) }
            }

            exhaustReceiveSessions(runtime)
            // A subscriber that starts after the failure receives it too.
            assertEquals(4uL, runtime.receiveFailure.first()?.code)
            ordinaryClose(runtime)
            assertEquals(4uL, runtime.receiveFailure.value?.code)

            // Only the next explicit mesh attempt clears the warning.
            runtime.clearReceiveFailure()
            assertNull(runtime.receiveFailure.first())
            exhaustReceiveSessions(runtime)
            assertEquals(4uL, runtime.receiveFailure.first()?.code)
            assertEquals(if (stallCollector) 1 else 0, observed)
        } finally {
            observer?.cancel()
            runtime.close()
            dir.deleteRecursively()
        }
    }
}
