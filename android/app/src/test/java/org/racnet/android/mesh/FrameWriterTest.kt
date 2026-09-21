package org.racnet.android.mesh

import java.io.ByteArrayOutputStream
import java.io.IOException
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class FrameWriterTest {
    @Test
    fun `terminal close drains frames in order even before writer starts`() = runTest {
        val output = ByteArrayOutputStream()
        val chunks = mutableListOf<Int>()
        var closes = 0
        lateinit var writer: FrameWriter
        writer = FrameWriter(backgroundScope, { 23 }, {
            assertEquals(0, closes)
            chunks.add(it.size)
            output.write(it)
        }, {
            closes++
            writer.cancel()
        })
        val preceding = ByteArray(50) { it.toByte() }
        val terminal = byteArrayOf(90, 91, 92)
        assertTrue(writer.enqueue(listOf(preceding, terminal)))
        writer.finish()
        assertFalse(writer.enqueue(listOf(byteArrayOf(1))))
        val job = backgroundScope.launch { writer.run() }
        runCurrent()
        assertTrue(job.isCompleted)
        assertEquals(listOf(23, 23, 4, 3), chunks)
        assertArrayEquals(preceding + terminal, output.toByteArray())
        assertEquals(1, closes)
        advanceTimeBy(2_001)
        runCurrent()
        assertEquals(1, closes)
    }

    @Test
    fun `stalled terminal write forces transport close at deadline`() = runTest {
        var started = false
        var closes = 0
        lateinit var writer: FrameWriter
        writer = FrameWriter(backgroundScope, { 23 }, {
            started = true
            awaitCancellation()
        }, {
            closes++
            writer.cancel()
        }, drainTimeoutMs = 100)
        assertTrue(writer.enqueue(listOf(byteArrayOf(1))))
        val job = backgroundScope.launch { writer.run() }
        writer.finish()
        runCurrent()
        assertTrue(started)
        advanceTimeBy(99)
        runCurrent()
        assertEquals(0, closes)
        advanceTimeBy(1)
        runCurrent()
        assertEquals(1, closes)
        job.cancel()
        runCurrent()
        assertEquals(1, closes)
    }

    @Test
    fun `socket write failure closes once without sending later frames`() = runTest {
        var writes = 0
        var closes = 0
        lateinit var writer: FrameWriter
        writer = FrameWriter(backgroundScope, { 23 }, {
            writes++
            throw IOException("closed test transport")
        }, {
            closes++
            writer.cancel()
        })
        assertTrue(writer.enqueue(listOf(byteArrayOf(1), byteArrayOf(2))))
        writer.finish()
        backgroundScope.launch { writer.run() }
        runCurrent()
        assertEquals(1, writes)
        assertEquals(1, closes)
    }

    @Test
    fun `queue bounds include the frame currently being written`() = runTest {
        lateinit var writer: FrameWriter
        writer = FrameWriter(backgroundScope, { 23 }, { awaitCancellation() }, {
            writer.cancel()
        }, maxQueuedBytes = 3)
        assertTrue(writer.enqueue(listOf(byteArrayOf(1, 2, 3))))
        backgroundScope.launch { writer.run() }
        runCurrent()
        assertFalse(writer.enqueue(listOf(byteArrayOf(4))))
        writer.cancel()
    }
}
