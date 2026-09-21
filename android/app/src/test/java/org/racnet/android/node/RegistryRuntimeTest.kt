package org.racnet.android.node

import java.io.File
import java.nio.file.Files
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.*
import org.junit.Test
import org.racnet.android.mesh.ConnectionRegistry
import org.racnet.android.mesh.RegistryConnection
import org.racnet.android.metrics.LinkMetrics
import org.racnet.android.policy.DuplicateLinkPolicy
import uniffi.racnet_core.CloseCause
import uniffi.racnet_core.Identity

/** Real core handshakes drive the registry without Bluetooth or event routing. */
class RegistryRuntimeTest {
    private class Pipe(
        private val registry: ConnectionRegistry,
        override val initiator: Boolean,
    ) : LinkTransport, RegistryConnection {
        override var linkId = ULong.MAX_VALUE
        override val address = "test"
        override val metrics = LinkMetrics(address, initiator)
        val frames = ArrayDeque<ByteArray>()
        var closed = false
        override fun writeFrames(frames: List<ByteArray>) {
            if (!closed) this.frames.addAll(frames)
        }
        override fun onEstablished(remoteFingerprint: ByteArray) {
            registry.onEstablished(linkId, remoteFingerprint)
        }
        override fun closeFromCore(cause: CloseCause, drainWrites: Boolean) { shutdown(false) }
        override fun shutdown(reportToCore: Boolean) {
            if (closed) return
            closed = true
            frames.clear()
            registry.remove(this)
        }
    }

    private data class Edge(val a: NodeRuntime, val ap: Pipe, val b: NodeRuntime, val bp: Pipe)

    private suspend fun connect(
        a: NodeRuntime, ar: ConnectionRegistry, b: NodeRuntime, br: ConnectionRegistry,
    ): Edge {
        val ap = Pipe(ar, true)
        val bp = Pipe(br, false)
        ap.linkId = a.connect(ap).linkId
        ar.register(ap)
        bp.linkId = b.accept(byteArrayOf(1, 2, 3, 4, 5, 6), bp)!!.linkId
        br.register(bp)
        return Edge(a, ap, b, bp)
    }

    private suspend fun pump(vararg edges: Edge) {
        repeat(1000) {
            var progressed = false
            for (edge in edges) {
                if (edge.ap.closed || edge.bp.closed) continue
                if (edge.ap.frames.isNotEmpty()) {
                    edge.b.onBytes(edge.bp.linkId, edge.ap.frames.removeFirst())
                    progressed = true
                }
                if (edge.ap.closed || edge.bp.closed) continue
                if (edge.bp.frames.isNotEmpty()) {
                    edge.a.onBytes(edge.ap.linkId, edge.bp.frames.removeFirst())
                    progressed = true
                }
            }
            if (!progressed) return
        }
        fail("handshakes and sync did not quiesce")
    }

    private suspend fun emitClosedEvent(runtime: NodeRuntime) {
        val transport = object : LinkTransport {
            override fun writeFrames(frames: List<ByteArray>) {}
            override fun onEstablished(remoteFingerprint: ByteArray) {}
            override fun closeFromCore(cause: CloseCause, drainWrites: Boolean) {}
        }
        runtime.onTransportClosed(runtime.connect(transport).linkId)
    }

    private fun identity(seed: Int) =
        Identity(ByteArray(32) { seed.toByte() }, ByteArray(32) { (seed + 32).toByte() })

    @Test fun registryAndDuplicateResolutionWorkWithoutEventCollectors() = verifyRegistry(false)

    @Test fun registryAndDuplicateResolutionSurviveStalledEventCollectors() = verifyRegistry(true)

    @OptIn(kotlinx.coroutines.ExperimentalCoroutinesApi::class)
    private fun verifyRegistry(stallCollectors: Boolean) = runTest {
        val dir = Files.createTempDirectory("racnet-registry").toFile()
        val a = NodeRuntime(identity(10), File(dir, "a").path) { 0uL }
        val b = NodeRuntime(identity(11), File(dir, "b").path) { 0uL }
        val records = mutableListOf<String>()
        val ar = ConnectionRegistry(a.fingerprint, { 100L }) { event, _ -> records.add(event) }
        val br = ConnectionRegistry(b.fingerprint, { 100L }) { event, _ -> records.add(event) }
        val edges = mutableListOf<Edge>()
        var observed = 0
        val observers = if (stallCollectors) listOf(a, b).map { runtime ->
            backgroundScope.launch(start = CoroutineStart.UNDISPATCHED) {
                runtime.events.collect {
                    observed++
                    awaitCancellation()
                }
            }
        } else emptyList()
        try {
            if (stallCollectors) {
                emitClosedEvent(a)
                emitClosedEvent(b)
                runCurrent()
                assertEquals(2, observed)
                // More than the 1024-event diagnostic buffer on both runtimes.
                repeat(1100) {
                    emitClosedEvent(a)
                    emitClosedEvent(b)
                }
            }

            edges += connect(a, ar, b, br)
            pump(*edges.toTypedArray())
            assertEquals(1, ar.peers.value.size)
            assertEquals(1, br.peers.value.size)

            // Cross the roles, as two simultaneously dialing phones can do.
            edges += connect(b, br, a, ar)
            pump(*edges.toTypedArray())
            assertEquals(1, ar.peers.value.size)
            assertEquals(1, br.peers.value.size)
            assertEquals(DuplicateLinkPolicy.compareFingerprints(a.fingerprint, b.fingerprint) < 0,
                ar.peers.value.single().initiator)
            assertEquals(!ar.peers.value.single().initiator, br.peers.value.single().initiator)
            assertTrue(records.contains("duplicate_link_closed"))
            assertEquals(if (stallCollectors) 2 else 0, observed)
        } finally {
            observers.forEach { it.cancel() }
            for (edge in edges) {
                edge.a.onTransportClosed(edge.ap.linkId)
                edge.b.onTransportClosed(edge.bp.linkId)
            }
            a.close()
            b.close()
            dir.deleteRecursively()
        }
    }
}
