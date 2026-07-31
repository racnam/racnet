package org.racnet.android.mesh

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class FrameChunkerTest {

    @Test
    fun `small frames pass through whole`() {
        val frame = ByteArray(100) { it.toByte() }
        val chunks = FrameChunker.chunk(frame, 512)
        assertEquals(1, chunks.size)
        assertArrayEquals(frame, chunks[0])
    }

    @Test
    fun `exact multiples split with no runt chunk`() {
        val frame = ByteArray(1_024) { it.toByte() }
        val chunks = FrameChunker.chunk(frame, 256)
        assertEquals(4, chunks.size)
        assertTrue(chunks.all { it.size == 256 })
        assertArrayEquals(frame, chunks.reduce { acc, c -> acc + c })
    }

    @Test
    fun `one over a multiple yields a single-byte tail`() {
        val frame = ByteArray(257) { it.toByte() }
        val chunks = FrameChunker.chunk(frame, 256)
        assertEquals(2, chunks.size)
        assertEquals(256, chunks[0].size)
        assertEquals(1, chunks[1].size)
        assertArrayEquals(frame, chunks[0] + chunks[1])
    }

    @Test
    fun `a maximum frame reassembles exactly`() {
        // Largest wire frame: 63 488 padded + 16 tag + 2 length prefix.
        val frame = ByteArray(63_506) { (it % 251).toByte() }
        val chunks = FrameChunker.chunk(frame, 512)
        assertArrayEquals(frame, chunks.reduce { acc, c -> acc + c })
        assertTrue(chunks.dropLast(1).all { it.size == 512 })
    }

    @Test
    fun `nonsense MTUs fall back to the BLE floor`() {
        val frame = ByteArray(100) { it.toByte() }
        for (mtu in listOf(0, -5, 1)) {
            val chunks = FrameChunker.chunk(frame, mtu)
            assertTrue(chunks.all { it.size <= FrameChunker.MIN_MTU })
            assertArrayEquals(frame, chunks.reduce { acc, c -> acc + c })
        }
    }
}
