package org.racnet.android.ble

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class BleConstantsTest {

    @Test
    fun `psm encodes as u16 little-endian per §9-1-3`() {
        assertArrayEquals(byteArrayOf(0x80.toByte(), 0x00), BleConstants.psmToBytes(0x0080))
        assertArrayEquals(byteArrayOf(0x35, 0x01), BleConstants.psmToBytes(0x0135))
        assertArrayEquals(
            byteArrayOf(0xFF.toByte(), 0xFF.toByte()),
            BleConstants.psmToBytes(0xFFFF),
        )
    }

    @Test
    fun `psm decode round-trips and rejects malformed reads`() {
        for (psm in listOf(1, 0x80, 0x135, 0xFFFF)) {
            assertEquals(psm, BleConstants.psmFromBytes(BleConstants.psmToBytes(psm)))
        }
        assertNull(BleConstants.psmFromBytes(null))
        assertNull(BleConstants.psmFromBytes(ByteArray(0)))
        assertNull(BleConstants.psmFromBytes(ByteArray(1)))
        assertNull(BleConstants.psmFromBytes(ByteArray(3)))
        assertNull("PSM zero is invalid", BleConstants.psmFromBytes(ByteArray(2)))
    }

    @Test
    fun `ble addresses become the 6 opaque limiter bytes`() {
        assertArrayEquals(
            byteArrayOf(
                0xAA.toByte(), 0xBB.toByte(), 0xCC.toByte(),
                0x00, 0x11, 0xFF.toByte(),
            ),
            BleConstants.addressToBytes("AA:BB:CC:00:11:FF"),
        )
        assertNull(BleConstants.addressToBytes("not-an-address"))
        assertNull(BleConstants.addressToBytes("AA:BB:CC:00:11"))
        assertNull(BleConstants.addressToBytes("AA:BB:CC:00:11:GG"))
        assertNull(BleConstants.addressToBytes("AA:BB:CC:00:11:F"))
    }
}
