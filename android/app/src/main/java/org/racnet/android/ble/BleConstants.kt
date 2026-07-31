package org.racnet.android.ble

import java.util.UUID

/**
 * The BLE binding constants of PROTOCOL.md §9.1 and their byte codecs.
 * The codecs are pure functions so they are unit-testable off-device.
 */
object BleConstants {

    /** §9.1.2: the advertised service UUID — the entire advertisement. */
    val SERVICE_UUID: UUID = UUID.fromString("eb254e0b-8c00-44ef-855d-603aa62f6597")

    /** §9.1.3: the read-only characteristic carrying the L2CAP PSM. */
    val PSM_CHARACTERISTIC_UUID: UUID = UUID.fromString("d52f8c73-c151-4132-b124-d6af600667f5")

    /** §9.1.3: PSM as a 2-octet little-endian unsigned integer. */
    fun psmToBytes(psm: Int): ByteArray {
        require(psm in 1..0xFFFF) { "PSM out of u16 range: $psm" }
        return byteArrayOf((psm and 0xFF).toByte(), ((psm shr 8) and 0xFF).toByte())
    }

    /** Decodes a §9.1.3 PSM value; null on any malformed read. */
    fun psmFromBytes(bytes: ByteArray?): Int? {
        if (bytes == null || bytes.size != 2) return null
        val psm = (bytes[0].toInt() and 0xFF) or ((bytes[1].toInt() and 0xFF) shl 8)
        return if (psm == 0) null else psm
    }

    /**
     * §9.1.6: the observed BLE device address ("AA:BB:CC:DD:EE:FF") as
     * the 6 opaque bytes keying the handshake rate limiter. Null if the
     * platform hands us something that is not a BLE address.
     */
    fun addressToBytes(address: String): ByteArray? {
        val parts = address.split(":")
        if (parts.size != 6) return null
        val bytes = ByteArray(6)
        for ((i, part) in parts.withIndex()) {
            val value = part.toIntOrNull(16) ?: return null
            if (value !in 0..0xFF || part.length != 2) return null
            bytes[i] = value.toByte()
        }
        return bytes
    }
}
