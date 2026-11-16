package dev.idecraft.mobile

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
import uniffi.idecraft_mobile.SecretVault
import uniffi.idecraft_mobile.VaultException

/**
 * This device's key and its pinned hosts (ADR-0029 §4, SPEC-0326 behaviour 4).
 *
 * An AES-256 key that never leaves the Android Keystore encrypts each value with AES-GCM; only the
 * ciphertext is written, to `noBackupFilesDir`, which no backup or device transfer copies (the
 * manifest turns backups off as well). The key needs an unlocked device, is bound to this app and
 * this device, and is kept in StrongBox where the device has one.
 *
 * A value whose key is gone or that fails to decrypt is an error and stays on disk: replacing it
 * silently would drop every pairing.
 */
class KeystoreVault(context: Context) : SecretVault {
    private val folder = File(context.noBackupFilesDir, "vault")

    private fun failed(reason: String): VaultException = VaultException.Failed(reason)

    private fun keyStore(): KeyStore =
        try {
            KeyStore.getInstance(KEYSTORE).apply { load(null) }
        } catch (e: Exception) {
            throw failed("Android Keystore unavailable (${e.javaClass.simpleName})")
        }

    private fun existingKey(): SecretKey? =
        try {
            keyStore().getKey(ALIAS, null) as SecretKey?
        } catch (e: VaultException) {
            throw e
        } catch (e: Exception) {
            throw failed("Keystore key unreadable (${e.javaClass.simpleName})")
        }

    private fun newKey(strongBox: Boolean): SecretKey {
        val spec =
            KeyGenParameterSpec.Builder(
                    ALIAS,
                    KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
                )
                .setKeySize(256)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setRandomizedEncryptionRequired(true)
                .setUnlockedDeviceRequired(true)
                .setIsStrongBoxBacked(strongBox)
                .build()
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE)
            .apply { init(spec) }
            .generateKey()
    }

    private fun keyForWriting(): SecretKey =
        existingKey()
            ?: try {
                try {
                    newKey(strongBox = true)
                } catch (_: StrongBoxUnavailableException) {
                    newKey(strongBox = false)
                } catch (_: java.security.ProviderException) {
                    // Some StrongBox chips refuse a parameter they do not support; the
                    // Keystore's default (TEE where present) keeps every other property.
                    newKey(strongBox = false)
                }
            } catch (e: Exception) {
                throw failed("cannot create the Keystore key (${e.javaClass.simpleName})")
            }

    private fun file(name: String): File {
        if (!NAME.matches(name)) throw failed("not a vault name")
        return File(folder, "$name.bin")
    }

    private fun aad(name: String) = "idecraft-vault:$FORMAT:$name".toByteArray()

    @Synchronized
    override fun load(name: String): ByteArray? {
        val file = file(name)
        if (!file.exists()) return null
        val key =
            existingKey()
                ?: throw failed(
                    "the Keystore key for $name is gone; the stored value is left in place"
                )
        val bytes =
            try {
                file.readBytes()
            } catch (e: Exception) {
                throw failed("cannot read $name (${e.javaClass.simpleName})")
            }
        if (bytes.size < 1 + IV_BYTES + TAG_BITS / 8 || bytes[0] != FORMAT) {
            throw failed("$name is damaged")
        }
        return try {
            Cipher.getInstance(TRANSFORMATION)
                .apply {
                    init(
                        Cipher.DECRYPT_MODE,
                        key,
                        GCMParameterSpec(TAG_BITS, bytes, 1, IV_BYTES),
                    )
                    updateAAD(aad(name))
                }
                .doFinal(bytes, 1 + IV_BYTES, bytes.size - 1 - IV_BYTES)
        } catch (e: Exception) {
            throw failed("cannot decrypt $name (${e.javaClass.simpleName})")
        }
    }

    @Synchronized
    override fun save(name: String, value: ByteArray) {
        val file = file(name)
        val key = keyForWriting()
        val sealed =
            try {
                val cipher =
                    Cipher.getInstance(TRANSFORMATION).apply {
                        init(Cipher.ENCRYPT_MODE, key)
                        updateAAD(aad(name))
                    }
                val iv = cipher.iv
                if (iv.size != IV_BYTES) throw IllegalStateException("IV size")
                byteArrayOf(FORMAT) + iv + cipher.doFinal(value)
            } catch (e: Exception) {
                throw failed("cannot encrypt $name (${e.javaClass.simpleName})")
            } finally {
                value.fill(0)
            }
        try {
            folder.mkdirs()
            val temp = File(folder, "$name.tmp")
            temp.writeBytes(sealed)
            if (!temp.renameTo(file)) throw IllegalStateException("rename")
        } catch (e: Exception) {
            throw failed("cannot write $name (${e.javaClass.simpleName})")
        }
    }

    @Synchronized
    override fun delete(name: String) {
        val file = file(name)
        if (file.exists() && !file.delete()) throw failed("cannot delete $name")
    }

    companion object {
        internal const val KEYSTORE = "AndroidKeyStore"
        internal const val ALIAS = "dev.idecraft.mobile.vault"
        private const val TRANSFORMATION = "AES/GCM/NoPadding"
        private const val IV_BYTES = 12
        private const val TAG_BITS = 128
        private const val FORMAT: Byte = 1
        private val NAME = Regex("[a-z0-9_-]{1,64}")
    }
}
