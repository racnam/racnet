package org.racnet.android.node

import java.io.File
import java.nio.file.Files
import kotlinx.coroutines.test.runTest
import org.junit.Assert.*
import org.junit.Test
import uniffi.racnet_core.CloseCause
import uniffi.racnet_core.Identity

/** Drives the real Rust/JNA core through the same runtime used by BLE. */
class NodeRuntimeTest {
    private class Pipe : LinkTransport {
        val frames = ArrayDeque<ByteArray>()
        var closed = false
        override fun writeFrames(frames: List<ByteArray>) { this.frames.addAll(frames) }
        override fun closeFromCore(cause: CloseCause) { closed = true }
    }
    private data class Edge(val a: NodeRuntime, val aid: ULong, val ab: Pipe, val b: NodeRuntime, val bid: ULong, val ba: Pipe)
    private suspend fun edge(a: NodeRuntime, b: NodeRuntime): Edge {
        val ab = Pipe()
        val ba = Pipe()
        val aid = a.connect(ab).linkId
        val bid = b.accept(byteArrayOf(1, 2, 3, 4, 5, 6), ba)!!.linkId
        return Edge(a, aid, ab, b, bid, ba)
    }
    private suspend fun pump(vararg edges: Edge) {
        repeat(10000) {
            var progressed = false
            for (edge in edges) {
                if (edge.ab.frames.isNotEmpty()) {
                    edge.b.onBytes(edge.bid, edge.ab.frames.removeFirst())
                    progressed = true
                }
                if (edge.ba.frames.isNotEmpty()) {
                    edge.a.onBytes(edge.aid, edge.ba.frames.removeFirst())
                    progressed = true
                }
                assertFalse("a transport closed", edge.ab.closed)
                assertFalse("b transport closed", edge.ba.closed)
            }
            if (!progressed) return
        }
        fail("sync did not quiesce")
    }
    private fun identity(seed: Int) = Identity(ByteArray(32) { seed.toByte() }, ByteArray(32) { (seed + 32).toByte() })

    @Test fun rapidPostsRelayAcrossThreeNodesAndSurviveRestart() = runTest {
        val dir = Files.createTempDirectory("racnet-runtime").toFile()
        val a = NodeRuntime(identity(1), File(dir, "a").path) { 0uL }
        val b = NodeRuntime(identity(2), File(dir, "b").path) { 0uL }
        val c = NodeRuntime(identity(3), File(dir, "c").path) { 0uL }
        val ab = edge(a, b)
        val bc = edge(b, c)
        try {
            pump(ab, bc)
            repeat(100) { a.createEntry(1uL, "message $it".toByteArray()) }
            pump(ab, bc)
            assertEquals(100uL, a.entryCount.value)
            assertEquals(100uL, b.entryCount.value)
            assertEquals(100uL, c.entryCount.value)
            assertEquals(a.entries().map { it.id.toList() }, c.entries().map { it.id.toList() })
            c.createEntry(1uL, "reply from c".toByteArray())
            pump(ab, bc)
            assertEquals(101uL, a.entryCount.value)
        } finally {
            a.onTransportClosed(ab.aid)
            b.onTransportClosed(ab.bid)
            b.onTransportClosed(bc.aid)
            c.onTransportClosed(bc.bid)
            a.close(); b.close(); c.close()
        }
        try {
            val restarted = NodeRuntime(identity(3), File(dir, "c").path) { 0uL }
            try {
                assertEquals(101uL, restarted.entryCount.value)
                assertTrue(restarted.entries().any { it.payload.contentEquals("reply from c".toByteArray()) })
            } finally { restarted.close() }
        } finally { dir.deleteRecursively() }
    }
}
