import SwiftUI

@main
struct IDECraftApp: App {
    @StateObject private var model = AppModel()

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(model)
                .task {
                    model.refreshHosts()
                    #if DEBUG
                        await SelfTest.runIfAsked(model)
                    #endif
                }
        }
    }
}

struct ContentView: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        NavigationStack {
            Group {
                switch model.screen {
                case .pair: PairView()
                case .digits(let pairing): DigitsView(pairing: pairing)
                case .open(let address): OpenView(address: address)
                case .files(let project): FolderView(project: project, path: ".")
                }
            }
            .disabled(model.busy)
            .overlay { if model.busy { ProgressView() } }
        }
    }
}

private struct ErrorRow: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        if let error = model.error {
            Section { Text(error).foregroundStyle(.red) }
        }
    }
}

struct PairView: View {
    @EnvironmentObject private var model: AppModel
    @State private var address = ""
    @State private var code = ""

    var body: some View {
        Form {
            ErrorRow()
            Section("Pair with a computer") {
                TextField("host:port, or an idecraft-pair:// link", text: $address)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                TextField("Pairing code", text: $code)
                    .textInputAutocapitalization(.characters)
                    .autocorrectionDisabled()
                Button("Pair") { Task { await model.begin(address: address, secret: code) } }
                    .disabled(address.isEmpty)
            }
            if !model.hosts.isEmpty {
                Section("Paired") {
                    ForEach(model.hosts, id: \.address) { host in
                        Button {
                            model.screen = .open(address: host.address)
                        } label: {
                            VStack(alignment: .leading) {
                                Text(host.address)
                                Text(host.fingerprint).font(.caption.monospaced())
                            }
                        }
                    }
                }
            }
        }
        .navigationTitle("IDECraft")
    }
}

struct DigitsView: View {
    @EnvironmentObject private var model: AppModel
    let pairing: PairingInProgress

    var body: some View {
        Form {
            ErrorRow()
            Section("Compare with the computer") {
                Text(pairing.digits())
                    .font(.largeTitle.monospacedDigit())
                    .accessibilityIdentifier("digits")
                Text("Host key \(pairing.hostFingerprint())").font(.caption.monospaced())
            }
            Section {
                Button("They match") { Task { await model.confirm(pairing) } }
                Button("They differ", role: .destructive) { model.decline(pairing) }
            }
        }
        .navigationTitle("Check the digits")
    }
}

struct OpenView: View {
    @EnvironmentObject private var model: AppModel
    let address: String
    @State private var folder = ""

    var body: some View {
        Form {
            ErrorRow()
            Section("Project folder on \(address)") {
                TextField("/path/the/host/serves", text: $folder)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                Button("Open") { Task { await model.open(address: address, folder: folder) } }
                    .disabled(folder.isEmpty)
            }
        }
        .navigationTitle("Open project")
    }
}

struct FolderView: View {
    @EnvironmentObject private var model: AppModel
    let project: RemoteProject
    let path: String
    @State private var entries: [RemoteEntry] = []

    var body: some View {
        List {
            ErrorRow()
            ForEach(entries, id: \.path) { entry in
                switch entry.kind {
                case .directory:
                    NavigationLink(entry.name + "/") {
                        FolderView(project: project, path: entry.path)
                    }
                case .file:
                    NavigationLink(entry.name) { FileView(project: project, path: entry.path) }
                case .link:
                    Text(entry.name + " (link)").foregroundStyle(.secondary)
                }
            }
        }
        .navigationTitle(path == "." ? project.root() : path)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            if path == "." {
                Button("Close") { model.close(project) }
            }
        }
        .task {
            let project = self.project
            let path = self.path
            if let list = await model.run({ try project.list(path: path) }) {
                entries = list
            }
        }
    }
}

struct FileView: View {
    @EnvironmentObject private var model: AppModel
    let project: RemoteProject
    let path: String
    @State private var file: RemoteFile?

    var body: some View {
        ScrollView {
            if let file {
                Text(file.text ?? "This file is not text (\(file.size) bytes).")
                    .font(.body.monospaced())
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding()
                    .accessibilityIdentifier("file-text")
            } else if let error = model.error {
                Text(error).foregroundStyle(.red).padding()
            }
        }
        .navigationTitle(path)
        .navigationBarTitleDisplayMode(.inline)
        .task {
            let project = self.project
            let path = self.path
            file = await model.run { try project.readFile(path: path) }
        }
    }
}
