package dev.ferrisidea.mobile

import android.app.Application
import android.os.Build
import android.provider.Settings
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.AndroidViewModel
import kotlin.coroutines.cancellation.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import uniffi.ferris_idea_mobile.MobileCore
import uniffi.ferris_idea_mobile.PairedHostInfo
import uniffi.ferris_idea_mobile.PairingInProgress
import uniffi.ferris_idea_mobile.RemoteProject
import uniffi.ferris_idea_mobile.VaultException

/** What the app shows (SPEC-0326 behaviour 9). */
sealed interface Screen {
    data object Pair : Screen

    data class Digits(val pairing: PairingInProgress) : Screen

    data class Open(val address: String) : Screen

    /**
     * A project; `folders` is the path from the project's root to the folder shown, `file` an open
     * file. `root` is read once: the project's calls lock its session while a request runs.
     */
    data class Files(
        val project: RemoteProject,
        val root: String,
        val folders: List<String> = listOf("."),
        val file: String? = null,
    ) : Screen
}

/** The app's state. Calls into the core block, so they run on the IO dispatcher. */
class AppModel(application: Application) : AndroidViewModel(application) {
    var screen by mutableStateOf<Screen>(Screen.Pair)
    var busy by mutableStateOf(false)
        private set

    var error by mutableStateOf<String?>(null)
    var hosts by mutableStateOf<List<PairedHostInfo>>(emptyList())
        private set

    val vault = KeystoreVault(application)
    val core: MobileCore = MobileCore(vault, deviceName(application))

    /** The paired hosts; a vault that cannot be read is shown, not taken for "none paired". */
    suspend fun refreshHosts() {
        withContext(Dispatchers.IO) { runCatching { core.hosts() } }
            .onSuccess { hosts = it }
            .onFailure { e ->
                hosts = emptyList()
                error = if (e is Exception) describe(e) else e.toString()
            }
    }

    /** Runs `work` off the main thread; a failure is shown and `null` returned. */
    suspend fun <T> run(work: () -> T): T? {
        busy = true
        error = null
        return try {
            withContext(Dispatchers.IO) { work() }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            error = describe(e)
            null
        } finally {
            busy = false
        }
    }

    suspend fun begin(address: String, secret: String) {
        val link =
            when {
                secret.startsWith(LINK) -> secret
                address.startsWith(LINK) -> address
                else -> null
            }
        val pairing =
            run {
                if (link != null) core.beginPairingLink(link)
                else core.beginPairing(address, secret)
            } ?: return
        screen = Screen.Digits(pairing)
    }

    suspend fun confirm(pairing: PairingInProgress) {
        if (run { pairing.confirm() } == null) {
            pairing.close()
            screen = Screen.Pair
            return
        }
        refreshHosts()
        screen = Screen.Open(pairing.address())
        pairing.close()
    }

    suspend fun decline(pairing: PairingInProgress) {
        withContext(Dispatchers.IO) {
            pairing.decline()
            pairing.close()
        }
        screen = Screen.Pair
    }

    suspend fun open(address: String, folder: String) {
        val project = run { core.openProject(address, folder) } ?: return
        screen = Screen.Files(project, withContext(Dispatchers.IO) { project.root() })
    }

    suspend fun forget(address: String) {
        run { core.forgetHost(address) }
        refreshHosts()
    }

    suspend fun close(project: RemoteProject) {
        screen = Screen.Pair
        withContext(Dispatchers.IO) {
            project.disconnect()
            project.close()
        }
        refreshHosts()
    }

    /** One step back; `false` when there is nothing to go back to. */
    fun back(): Boolean {
        when (val s = screen) {
            is Screen.Pair -> return false
            is Screen.Digits -> {
                screen = Screen.Pair
                Thread { s.pairing.decline(); s.pairing.close() }.start()
            }
            is Screen.Open -> screen = Screen.Pair
            is Screen.Files ->
                when {
                    s.file != null -> screen = s.copy(file = null)
                    s.folders.size > 1 -> screen = s.copy(folders = s.folders.dropLast(1))
                    else -> {
                        screen = Screen.Pair
                        Thread { s.project.disconnect(); s.project.close() }.start()
                    }
                }
        }
        error = null
        return true
    }

    override fun onCleared() {
        core.close()
    }

    companion object {
        private const val LINK = "ferris-idea-pair://"

        private fun deviceName(app: Application): String =
            Settings.Global.getString(app.contentResolver, Settings.Global.DEVICE_NAME)
                ?: Build.MODEL
    }
}

/** The core's own message; a vault failure carries only its reason, never a value. */
fun describe(e: Exception): String =
    when (e) {
        is VaultException.Failed -> e.reason
        else -> e.message ?: e.javaClass.simpleName
    }
