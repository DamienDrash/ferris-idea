import SwiftUI
import UIKit

/// The app's state. Calls into the core block, so they run off the main thread.
@MainActor
final class AppModel: ObservableObject {
    enum Screen {
        case pair
        case digits(PairingInProgress)
        case open(address: String)
        case files(RemoteProject)
    }

    @Published var screen: Screen = .pair
    @Published var busy = false
    @Published var error: String?
    @Published var hosts: [PairedHostInfo] = []

    let core = MobileCore(vault: KeychainVault(), deviceName: UIDevice.current.name)

    func refreshHosts() {
        hosts = (try? core.hosts()) ?? []
    }

    /// Runs `work` off the main thread; a failure is shown and `nil` returned.
    func run<T: Sendable>(_ work: @escaping @Sendable () throws -> T) async -> T? {
        busy = true
        error = nil
        defer { busy = false }
        do {
            return try await Task.detached(priority: .userInitiated, operation: work).value
        } catch {
            self.error = describe(error)
            return nil
        }
    }

    func begin(address: String, secret: String) async {
        let core = self.core
        let isLink = secret.hasPrefix("idecraft-pair://") || address.hasPrefix("idecraft-pair://")
        let link = secret.hasPrefix("idecraft-pair://") ? secret : address
        guard
            let pairing = await run({
                isLink
                    ? try core.beginPairingLink(link: link)
                    : try core.beginPairing(address: address, code: secret)
            })
        else { return }
        screen = .digits(pairing)
    }

    func confirm(_ pairing: PairingInProgress) async {
        guard await run({ try pairing.confirm() }) != nil else {
            screen = .pair
            return
        }
        refreshHosts()
        screen = .open(address: pairing.address())
    }

    func decline(_ pairing: PairingInProgress) {
        pairing.decline()
        screen = .pair
    }

    func open(address: String, folder: String) async {
        let core = self.core
        guard let project = await run({ try core.openProject(address: address, folder: folder) })
        else { return }
        screen = .files(project)
    }

    func close(_ project: RemoteProject) {
        Task.detached { project.disconnect() }
        screen = .pair
        refreshHosts()
    }
}

/// The core's own message: every core error is a case carrying exactly one text.
func describe(_ error: Error) -> String {
    guard error is MobileError || error is VaultError,
        let payload = Mirror(reflecting: error).children.first?.value
    else { return error.localizedDescription }
    if let text = payload as? String { return text }
    return Mirror(reflecting: payload).children.first?.value as? String
        ?? error.localizedDescription
}
