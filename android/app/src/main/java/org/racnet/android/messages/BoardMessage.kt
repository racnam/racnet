package org.racnet.android.messages

import java.nio.ByteBuffer
import java.nio.charset.CodingErrorAction

/** Version-one public board messages; unknown entry kinds remain opaque. */
object BoardMessage {
    const val KIND: ULong = 1uL
    const val MAX_BYTES = 4096

    fun encode(text: String): ByteArray {
        val body = text.trim()
        require(body.isNotEmpty()) { "Write a message first." }
        val bytes = body.toByteArray(Charsets.UTF_8)
        require(bytes.size <= MAX_BYTES) { "Message is too long (maximum 4096 UTF-8 bytes)." }
        return bytes
    }

    fun decode(kind: ULong, payload: ByteArray): String? {
        if (kind != KIND || payload.size > MAX_BYTES) return null
        return try {
            Charsets.UTF_8.newDecoder()
                .onMalformedInput(CodingErrorAction.REPORT)
                .onUnmappableCharacter(CodingErrorAction.REPORT)
                .decode(ByteBuffer.wrap(payload)).toString().takeIf { it.isNotBlank() }
        } catch (e: java.nio.charset.CharacterCodingException) {
            null
        }
    }
}
