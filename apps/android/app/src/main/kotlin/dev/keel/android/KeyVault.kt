package dev.keel.android

import android.security.keystore.KeyGenParameterSpec
import dev.keel.android.kernel.Vault
import android.security.keystore.KeyProperties
import android.security.keystore.StrongBoxUnavailableException
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Wraps the device's secrets with an AES-256-GCM key the Android Keystore holds (ADR-0027): in
 * StrongBox where the device has one, else in its TEE, and never bound to a lock screen, which a
 * kiosk doesn't have. A secret it wraps is unwrapped again at once, to catch hardware that wraps
 * wrongly before anything depends on it.
 */
class KeyVault(private val alias: String = "keel.wrap") : Vault {
    private val keyStore = KeyStore.getInstance(KEYSTORE).apply { load(null) }

    /** [secret], wrapped: the 12-byte nonce, then the ciphertext and its tag. */
    override fun wrap(secret: ByteArray): ByteArray {
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, key())
        val wrapped = cipher.iv + cipher.doFinal(secret)
        check(unwrap(wrapped).contentEquals(secret)) { "the Keystore unwrapped a secret wrongly" }
        return wrapped
    }

    /** The secret [wrapped] holds. */
    override fun unwrap(wrapped: ByteArray): ByteArray {
        require(wrapped.size > NONCE) { "a wrapped secret is too short" }
        val cipher = Cipher.getInstance(TRANSFORMATION)
        // The key that wrapped it, never a new one: one the Keystore lost can't unwrap anything.
        // Found in review: a new key was made, and the failure was an opaque bad tag.
        val key = keyStore.getKey(alias, null) as SecretKey?
            ?: throw IllegalStateException("the Keystore no longer holds the key that wraps this register's secrets")
        cipher.init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(TAG_BITS, wrapped, 0, NONCE))
        return cipher.doFinal(wrapped, NONCE, wrapped.size - NONCE)
    }

    private fun key(): SecretKey = keyStore.getKey(alias, null) as SecretKey? ?: generate()

    private fun generate(): SecretKey {
        fun spec(strongBox: Boolean) = KeyGenParameterSpec.Builder(
            alias,
            KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
        )
            .setKeySize(256)
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setUserAuthenticationRequired(false)
            .setIsStrongBoxBacked(strongBox)
            .build()
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE)
        return try {
            generator.init(spec(strongBox = true))
            generator.generateKey()
        } catch (_: StrongBoxUnavailableException) {
            generator.init(spec(strongBox = false))
            generator.generateKey()
        }
    }

    private companion object {
        const val KEYSTORE = "AndroidKeyStore"
        const val TRANSFORMATION = "AES/GCM/NoPadding"
        const val NONCE = 12
        const val TAG_BITS = 128
    }
}
