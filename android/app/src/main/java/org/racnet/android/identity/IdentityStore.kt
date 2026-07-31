package org.racnet.android.identity

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.security.keystore.StrongBoxUnavailableException
import java.io.File
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import uniffi.racnet_core.Identity
import uniffi.racnet_core.generateIdentity

/**
 * Persists the device identity: the two seeds from the core, encrypted
 * with an AndroidKeyStore AES-GCM key and stored in [Context.getNoBackupFilesDir]
 * (ADR-0016). Losing the Keystore key rotates the identity, which the
 * protocol tolerates: there is no session state to lose, and entries
 * re-sync from peers.
 */
class IdentityStore(context: Context) {

    private val file = File(context.noBackupFilesDir, FILE_NAME)

    /** The persisted identity, or a freshly generated and persisted one. */
    fun loadOrCreate(): Identity {
        load()?.let { return it }
        val identity = generateIdentity()
        persist(identity)
        return identity
    }

    private fun load(): Identity? {
        if (!file.exists()) return null
        return try {
            val (iv, ciphertext) = IdentityEnvelope.unwrap(file.readBytes())
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(TAG_BITS, iv))
            val (noiseSeed, signingSeed) = IdentityEnvelope.split(cipher.doFinal(ciphertext))
            Identity(noiseSeed = noiseSeed, signingSeed = signingSeed)
        } catch (e: Exception) {
            // An unreadable identity file (lost Keystore key, corrupt
            // blob) means this device's identity is gone; rotate rather
            // than brick the mesh.
            file.delete()
            null
        }
    }

    private fun persist(identity: Identity) {
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, key())
        val ciphertext = cipher.doFinal(
            IdentityEnvelope.join(identity.noiseSeed, identity.signingSeed),
        )
        val blob = IdentityEnvelope.wrap(cipher.iv, ciphertext)
        // Write-then-rename so a crash never leaves a half-written file.
        val tmp = File(file.parentFile, "$FILE_NAME.tmp")
        tmp.writeBytes(blob)
        check(tmp.renameTo(file)) { "failed to move identity file into place" }
    }

    private fun key(): SecretKey {
        val store = KeyStore.getInstance(KEYSTORE).apply { load(null) }
        (store.getKey(KEY_ALIAS, null) as? SecretKey)?.let { return it }
        return try {
            generateKey(strongBox = true)
        } catch (e: StrongBoxUnavailableException) {
            generateKey(strongBox = false)
        }
    }

    private fun generateKey(strongBox: Boolean): SecretKey {
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE)
        generator.init(
            KeyGenParameterSpec.Builder(
                KEY_ALIAS,
                KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
            )
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .setIsStrongBoxBacked(strongBox)
                .build(),
        )
        return generator.generateKey()
    }

    private companion object {
        const val FILE_NAME = "identity.bin"
        const val KEYSTORE = "AndroidKeyStore"
        const val KEY_ALIAS = "racnet-identity"
        const val TRANSFORMATION = "AES/GCM/NoPadding"
        const val TAG_BITS = 128
    }
}
