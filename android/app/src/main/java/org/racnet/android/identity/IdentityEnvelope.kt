package org.racnet.android.identity

/**
 * Pure byte-level codec for the identity file (ADR-0016): the plaintext
 * is the two 32-byte seeds concatenated; the stored blob is the GCM IV
 * followed by the ciphertext. Pure functions so the envelope is
 * unit-testable without Android.
 */
object IdentityEnvelope {

    const val SEED_LENGTH = 32
    const val PLAINTEXT_LENGTH = 2 * SEED_LENGTH
    const val IV_LENGTH = 12

    /** Concatenates the seeds into the plaintext to encrypt. */
    fun join(noiseSeed: ByteArray, signingSeed: ByteArray): ByteArray {
        require(noiseSeed.size == SEED_LENGTH) { "noise seed must be $SEED_LENGTH bytes" }
        require(signingSeed.size == SEED_LENGTH) { "signing seed must be $SEED_LENGTH bytes" }
        return noiseSeed + signingSeed
    }

    /** Splits decrypted plaintext back into (noiseSeed, signingSeed). */
    fun split(plaintext: ByteArray): Pair<ByteArray, ByteArray> {
        require(plaintext.size == PLAINTEXT_LENGTH) {
            "identity plaintext must be $PLAINTEXT_LENGTH bytes, was ${plaintext.size}"
        }
        return Pair(
            plaintext.copyOfRange(0, SEED_LENGTH),
            plaintext.copyOfRange(SEED_LENGTH, PLAINTEXT_LENGTH),
        )
    }

    /** Frames IV and ciphertext into the stored blob. */
    fun wrap(iv: ByteArray, ciphertext: ByteArray): ByteArray {
        require(iv.size == IV_LENGTH) { "GCM IV must be $IV_LENGTH bytes" }
        require(ciphertext.isNotEmpty()) { "empty ciphertext" }
        return iv + ciphertext
    }

    /** Splits a stored blob back into (iv, ciphertext). */
    fun unwrap(blob: ByteArray): Pair<ByteArray, ByteArray> {
        require(blob.size > IV_LENGTH) { "identity blob too short: ${blob.size} bytes" }
        return Pair(
            blob.copyOfRange(0, IV_LENGTH),
            blob.copyOfRange(IV_LENGTH, blob.size),
        )
    }
}
