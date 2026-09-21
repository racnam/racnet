package org.racnet.android.mesh

import java.io.IOException
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/** Ordered, bounded writes with a deadline for draining a terminal frame. */
internal class FrameWriter(
    private val scope: CoroutineScope,
    private val maxTransmitSize: () -> Int,
    private val writeChunk: suspend (ByteArray) -> Unit,
    private val onClosed: () -> Unit,
    private val maxQueuedBytes: Long = 72L * 1024 * 1024,
    private val drainTimeoutMs: Long = 2_000,
) {
    private val frames = Channel<ByteArray>(Channel.UNLIMITED)
    private val queuedBytes = AtomicLong(0)
    private val closed = AtomicBoolean(false)
    private var finishing = false
    private var deadline: Job? = null

    @Synchronized
    fun enqueue(batch: List<ByteArray>): Boolean {
        if (finishing || closed.get()) return false
        for (frame in batch) {
            val size = frame.size.toLong()
            if (queuedBytes.addAndGet(size) > maxQueuedBytes) {
                queuedBytes.addAndGet(-size)
                return false
            }
            if (!frames.trySend(frame).isSuccess) {
                queuedBytes.addAndGet(-size)
                return false
            }
        }
        return true
    }

    /** Stop accepting frames; let the writer flush its queue before closing. */
    @Synchronized
    fun finish() {
        if (finishing || closed.get()) return
        finishing = true
        frames.close()
        // A blocking Bluetooth write is released by closing the socket, not
        // coroutine cancellation. The deadline therefore invokes teardown.
        deadline = scope.launch {
            delay(drainTimeoutMs)
            closeTransport()
        }
    }

    @Synchronized
    fun cancel() {
        finishing = true
        frames.cancel()
        deadline?.cancel()
    }

    suspend fun run() {
        try {
            for (frame in frames) {
                for (chunk in FrameChunker.chunk(frame, maxTransmitSize())) {
                    writeChunk(chunk)
                }
                queuedBytes.addAndGet(-frame.size.toLong())
            }
        } catch (e: IOException) {
            // Normal transport death; teardown releases both socket loops.
        } finally {
            closeTransport()
        }
    }

    private fun closeTransport() {
        if (closed.compareAndSet(false, true)) onClosed()
    }
}
