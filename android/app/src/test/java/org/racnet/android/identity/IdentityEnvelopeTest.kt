package org.racnet.android.identity

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class IdentityEnvelopeTest {

    private val noise = ByteArray(32) { 0x11 }
    private val signing = ByteArray(32) { 0x22 }

    @Test
    fun `join and split round-trip`() {
        val (noiseOut, signingOut) = IdentityEnvelope.split(
            IdentityEnvelope.join(noise, signing),
        )
        assertArrayEquals(noise, noiseOut)
        assertArrayEquals(signing, signingOut)
    }

    @Test
    fun `wrap and unwrap round-trip`() {
        val iv = ByteArray(12) { it.toByte() }
        val ciphertext = ByteArray(80) { (it * 3).toByte() }
        val (ivOut, ciphertextOut) = IdentityEnvelope.unwrap(
            IdentityEnvelope.wrap(iv, ciphertext),
        )
        assertArrayEquals(iv, ivOut)
        assertArrayEquals(ciphertext, ciphertextOut)
    }

    @Test
    fun `wrong seed lengths are rejected`() {
        assertThrows(IllegalArgumentException::class.java) {
            IdentityEnvelope.join(ByteArray(31), signing)
        }
        assertThrows(IllegalArgumentException::class.java) {
            IdentityEnvelope.join(noise, ByteArray(33))
        }
    }

    @Test
    fun `truncated plaintext and blobs are rejected`() {
        assertThrows(IllegalArgumentException::class.java) {
            IdentityEnvelope.split(ByteArray(63))
        }
        assertThrows(IllegalArgumentException::class.java) {
            IdentityEnvelope.unwrap(ByteArray(IdentityEnvelope.IV_LENGTH))
        }
        assertThrows(IllegalArgumentException::class.java) {
            IdentityEnvelope.wrap(ByteArray(11), ByteArray(10))
        }
        assertThrows(IllegalArgumentException::class.java) {
            IdentityEnvelope.wrap(ByteArray(12), ByteArray(0))
        }
    }
}
