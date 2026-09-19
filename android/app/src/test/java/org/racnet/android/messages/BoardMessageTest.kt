package org.racnet.android.messages

import org.junit.Assert.*
import org.junit.Test

class BoardMessageTest {
    @Test fun unicodeRoundTrip() {
        val text = "Hello, 世界 🌍"
        assertEquals(text, BoardMessage.decode(1uL, BoardMessage.encode(" $text ")))
    }
    @Test fun rejectsUnknownKindsMalformedUtf8AndOversizedPayloads() {
        assertNull(BoardMessage.decode(0uL, byteArrayOf(65)))
        assertNull(BoardMessage.decode(1uL, byteArrayOf(0xc3.toByte())))
        assertNull(BoardMessage.decode(1uL, ByteArray(4097) { 65 }))
        assertThrows(IllegalArgumentException::class.java) { BoardMessage.encode("   ") }
        assertThrows(IllegalArgumentException::class.java) { BoardMessage.encode("🌍".repeat(1025)) }
        assertEquals(4096, BoardMessage.encode("🌍".repeat(1024)).size)
    }
}
