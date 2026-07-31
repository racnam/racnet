package org.racnet.android.mesh

/**
 * Splits outgoing frames into SDU-sized writes: Android L2CAP CoC
 * sockets reject or truncate writes larger than the outgoing MTU on
 * several stacks, and a frame can be up to ~63.5 KiB. Frame boundaries
 * carry no meaning on the stream (§1.1), so chunking is purely local.
 */
object FrameChunker {

    /** A conservative floor when the socket reports a nonsensical MTU. */
    const val MIN_MTU = 23

    fun chunk(frame: ByteArray, mtu: Int): List<ByteArray> {
        require(frame.isNotEmpty()) { "empty frame" }
        val size = mtu.coerceAtLeast(MIN_MTU)
        if (frame.size <= size) return listOf(frame)
        val chunks = ArrayList<ByteArray>((frame.size + size - 1) / size)
        var offset = 0
        while (offset < frame.size) {
            val end = (offset + size).coerceAtMost(frame.size)
            chunks.add(frame.copyOfRange(offset, end))
            offset = end
        }
        return chunks
    }
}
