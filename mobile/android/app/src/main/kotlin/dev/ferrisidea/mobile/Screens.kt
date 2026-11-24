package dev.ferrisidea.mobile

import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.launch
import uniffi.ferris_idea_mobile.EntryKind
import uniffi.ferris_idea_mobile.RemoteEntry
import uniffi.ferris_idea_mobile.RemoteFile
import uniffi.ferris_idea_mobile.RemoteProject

@Composable
fun App(model: AppModel) {
    Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
        Box(Modifier.safeDrawingPadding().imePadding()) {
            when (val s = model.screen) {
                is Screen.Pair -> PairScreen(model)
                is Screen.Digits -> DigitsScreen(model, s)
                is Screen.Open -> OpenScreen(model, s.address)
                is Screen.Files ->
                    if (s.file != null) FileScreen(model, s.project, s.file)
                    else FolderScreen(model, s)
            }
            if (model.busy) {
                CircularProgressIndicator(Modifier.align(Alignment.Center))
            }
        }
    }
}

@Composable
private fun Title(text: String, action: (@Composable () -> Unit)? = null) {
    Row(
        Modifier.fillMaxWidth().padding(16.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        Text(text, style = MaterialTheme.typography.titleLarge, modifier = Modifier.weight(1f))
        action?.invoke()
    }
}

@Composable
private fun ErrorText(model: AppModel) {
    model.error?.let {
        Text(
            it,
            color = MaterialTheme.colorScheme.error,
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp).testTag("error"),
        )
    }
}

@Composable
private fun PairScreen(model: AppModel) {
    val scope = rememberCoroutineScope()
    var address by rememberSaveable { mutableStateOf("") }
    var code by rememberSaveable { mutableStateOf("") }
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
        Title("Ferris IDEA")
        ErrorText(model)
        Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text("Pair with a computer", style = MaterialTheme.typography.titleMedium)
            OutlinedTextField(
                address,
                { address = it },
                label = { Text("host:port, or an ferris-idea-pair:// link") },
                singleLine = true,
                enabled = !model.busy,
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, autoCorrectEnabled = false),
                modifier = Modifier.fillMaxWidth().testTag("address"),
            )
            OutlinedTextField(
                code,
                { code = it },
                label = { Text("Pairing code") },
                singleLine = true,
                enabled = !model.busy,
                keyboardOptions =
                    KeyboardOptions(
                        capitalization = KeyboardCapitalization.Characters,
                        autoCorrectEnabled = false,
                        keyboardType = KeyboardType.Password,
                    ),
                modifier = Modifier.fillMaxWidth().testTag("code"),
            )
            Button(
                onClick = { scope.launch { model.begin(address, code) } },
                enabled = address.isNotBlank() && !model.busy,
            ) {
                Text("Pair")
            }
        }
        if (model.hosts.isNotEmpty()) {
            Text(
                "Paired",
                style = MaterialTheme.typography.titleMedium,
                modifier = Modifier.padding(16.dp),
            )
            for (host in model.hosts) {
                Row(
                    Modifier.fillMaxWidth()
                        .clickable(enabled = !model.busy) { model.screen = Screen.Open(host.address) }
                        .padding(horizontal = 16.dp, vertical = 8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Column(Modifier.weight(1f)) {
                        Text(host.address)
                        Text(
                            host.fingerprint,
                            fontFamily = FontFamily.Monospace,
                            style = MaterialTheme.typography.bodySmall,
                        )
                    }
                    TextButton(
                        onClick = { scope.launch { model.forget(host.address) } },
                        enabled = !model.busy,
                    ) {
                        Text("Forget")
                    }
                }
                HorizontalDivider()
            }
        }
    }
}

@Composable
private fun DigitsScreen(model: AppModel, s: Screen.Digits) {
    val scope = rememberCoroutineScope()
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
        Title("Check the digits")
        ErrorText(model)
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text("Compare with the computer", style = MaterialTheme.typography.titleMedium)
            Text(
                s.pairing.digits(),
                fontSize = 40.sp,
                fontFamily = FontFamily.Monospace,
                modifier = Modifier.testTag("digits"),
            )
            Text(
                "Host key ${s.pairing.hostFingerprint()}",
                fontFamily = FontFamily.Monospace,
                style = MaterialTheme.typography.bodySmall,
            )
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Button(
                    onClick = { scope.launch { model.confirm(s.pairing) } },
                    enabled = !model.busy,
                ) {
                    Text("They match")
                }
                OutlinedButton(
                    onClick = { scope.launch { model.decline(s.pairing) } },
                    enabled = !model.busy,
                ) {
                    Text("They differ", color = MaterialTheme.colorScheme.error)
                }
            }
        }
    }
}

@Composable
private fun OpenScreen(model: AppModel, address: String) {
    val scope = rememberCoroutineScope()
    var folder by rememberSaveable { mutableStateOf("") }
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
        Title("Open project")
        ErrorText(model)
        Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text("Project folder on $address", style = MaterialTheme.typography.titleMedium)
            OutlinedTextField(
                folder,
                { folder = it },
                label = { Text("/path/the/host/serves") },
                singleLine = true,
                enabled = !model.busy,
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, autoCorrectEnabled = false),
                modifier = Modifier.fillMaxWidth().testTag("folder"),
            )
            Button(
                onClick = { scope.launch { model.open(address, folder) } },
                enabled = folder.isNotBlank() && !model.busy,
            ) {
                Text("Open")
            }
        }
    }
}

@Composable
private fun FolderScreen(model: AppModel, s: Screen.Files) {
    val scope = rememberCoroutineScope()
    val path = s.folders.last()
    var entries by remember(s.project, path) { mutableStateOf<List<RemoteEntry>>(emptyList()) }
    LaunchedEffect(s.project, path) {
        model.run { s.project.list(path) }?.let { entries = it }
    }
    Column(Modifier.fillMaxSize()) {
        Title(if (path == ".") s.root else path) {
            if (path == ".") {
                TextButton(onClick = { scope.launch { model.close(s.project) } }) { Text("Close") }
            }
        }
        ErrorText(model)
        LazyColumn(Modifier.fillMaxSize().testTag("entries")) {
            items(entries, key = { it.path }) { entry ->
                val (label, onClick) =
                    when (entry.kind) {
                        EntryKind.DIRECTORY ->
                            entry.name + "/" to { model.screen = s.copy(folders = s.folders + entry.path) }
                        EntryKind.FILE -> entry.name to { model.screen = s.copy(file = entry.path) }
                        EntryKind.LINK -> "${entry.name} (link)" to null
                    }
                Text(
                    label,
                    color =
                        if (onClick == null) MaterialTheme.colorScheme.onSurfaceVariant
                        else MaterialTheme.colorScheme.onSurface,
                    modifier =
                        Modifier.fillMaxWidth()
                            .let { m -> if (onClick != null) m.clickable(enabled = !model.busy, onClick = onClick) else m }
                            .padding(horizontal = 16.dp, vertical = 14.dp),
                )
                HorizontalDivider()
            }
        }
    }
}

@Composable
private fun FileScreen(model: AppModel, project: RemoteProject, path: String) {
    var file by remember(project, path) { mutableStateOf<RemoteFile?>(null) }
    LaunchedEffect(project, path) { file = model.run { project.readFile(path) } }
    Column(Modifier.fillMaxSize()) {
        Title(path)
        ErrorText(model)
        file?.let { f ->
            SelectionContainer {
                Text(
                    f.text ?: "This file is not text (${f.size} bytes).",
                    fontFamily = FontFamily.Monospace,
                    style = MaterialTheme.typography.bodySmall,
                    modifier =
                        Modifier.fillMaxSize()
                            .verticalScroll(rememberScrollState())
                            .horizontalScroll(rememberScrollState())
                            .padding(16.dp)
                            .testTag("file-text"),
                )
            }
        }
    }
}
