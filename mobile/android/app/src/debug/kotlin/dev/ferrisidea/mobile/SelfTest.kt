package dev.ferrisidea.mobile

import android.content.Context
import android.content.Intent
import android.content.pm.ApplicationInfo
import android.os.Build
import android.security.keystore.KeyInfo
import android.util.Log
import java.io.File
import java.security.KeyStore
import javax.crypto.SecretKey
import javax.crypto.SecretKeyFactory

/**
 * The app's self-test (SPEC-0326 behaviour 9 and 10): the path of a person through the same model,
 * driven by intent extras from `scripts/mobile-selftest.py`. Debug builds only: the release source
 * set has a stub with no code that confirms digits by itself.
 */
object SelfTest {
    class Request(val address: String, val code: String, val folder: String, val file: String)

    private const val TAG = "FerrisIDEASelfTest"

    fun request(intent: Intent?): Request? {
        val extras = intent?.extras ?: return null
        return Request(
            extras.getString("selftest_address") ?: return null,
            extras.getString("selftest_code") ?: return null,
            extras.getString("selftest_folder") ?: return null,
            extras.getString("selftest_file") ?: return null,
        )
    }

    private fun say(line: String) {
        Log.i(TAG, "SELFTEST $line")
    }

    suspend fun run(model: AppModel, request: Request, context: Context) {
        model.begin(request.address, request.code)
        val pairing = (model.screen as? Screen.Digits)?.pairing
        if (pairing == null) {
            say("FAIL pair ${model.error ?: "?"}")
            return
        }
        say("digits=${pairing.digits()} host=${pairing.hostFingerprint()}")
        model.confirm(pairing)
        val paired = (model.screen as? Screen.Open)?.address
        if (paired == null) {
            say("FAIL confirm ${model.error ?: "?"}")
            return
        }
        keystoreFacts(model, context).forEach(::say)
        say("paired address=$paired vault_hosts=${model.hosts.size}")
        model.open(paired, request.folder)
        val files = model.screen as? Screen.Files
        if (files == null) {
            say("FAIL open ${model.error ?: "?"}")
            return
        }
        val project = files.project
        say("rights=${model.run { project.rights() }?.joinToString(",")}")
        val entries = model.run { project.list(".") }
        if (entries == null) {
            say("FAIL list ${model.error ?: "?"}")
            return
        }
        say("entries=${entries.joinToString(",") { it.name }}")
        val content = model.run { project.readFile(request.file) }
        if (content == null) {
            say("FAIL read ${model.error ?: "?"}")
            return
        }
        say("read path=${content.path} size=${content.size} first=${content.text?.take(60)?.replace('\n', ' ')}")
        // Leave the file on screen for the screenshot.
        model.screen = files.copy(file = request.file)
        say("PASS")
    }

    /** What the Keystore and the vault folder really hold; never a value. */
    private fun keystoreFacts(model: AppModel, context: Context): List<String> {
        val out = mutableListOf<String>()
        val key = KeyStore.getInstance(KeystoreVault.KEYSTORE).apply { load(null) }
            .getKey(KeystoreVault.ALIAS, null) as SecretKey?
        if (key == null) {
            out += "keystore key=missing"
        } else {
            val info =
                SecretKeyFactory.getInstance(key.algorithm, KeystoreVault.KEYSTORE)
                    .getKeySpec(key, KeyInfo::class.java) as KeyInfo
            val level =
                if (Build.VERSION.SDK_INT >= 31) {
                    when (info.securityLevel) {
                        0 -> "software"
                        1 -> "trusted_environment"
                        2 -> "strongbox"
                        else -> "unknown_or_${info.securityLevel}"
                    }
                } else if (@Suppress("DEPRECATION") info.isInsideSecureHardware) "secure_hardware"
                else "software"
            out +=
                "keystore key=${key.algorithm}-${info.keySize} level=$level " +
                    "modes=${info.blockModes.joinToString("+")} " +
                    "paddings=${info.encryptionPaddings.joinToString("+")} purposes=${info.purposes} " +
                    "user_auth=${info.isUserAuthenticationRequired} exportable=${key.encoded != null}"
        }
        val folder = File(context.noBackupFilesDir, "vault")
        for (name in listOf("identity", "pins")) {
            val file = File(folder, "$name.bin")
            val stored = runCatching { model.vault.load(name) }.getOrNull()
            val raw = if (file.exists()) file.readBytes() else ByteArray(0)
            val plain = stored != null && stored.isNotEmpty() && contains(raw, stored)
            stored?.fill(0)
            out += "vault $name file_bytes=${raw.size} value_bytes=${stored?.size} plaintext_on_disk=$plain"
        }
        val backup = context.applicationInfo.flags and ApplicationInfo.FLAG_ALLOW_BACKUP != 0
        out += "vault folder=noBackupFilesDir allow_backup=$backup " +
            "prefs=${File(context.dataDir, "shared_prefs").list()?.size ?: 0}"
        return out
    }

    private fun contains(haystack: ByteArray, needle: ByteArray): Boolean {
        if (needle.size > haystack.size) return false
        outer@ for (i in 0..haystack.size - needle.size) {
            for (j in needle.indices) if (haystack[i + j] != needle[j]) continue@outer
            return true
        }
        return false
    }
}
