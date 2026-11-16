#if DEBUG
    import Foundation
    import Security

    /// The app's self-test (SPEC-0326 behaviour 9): the path of a person through the same model, driven by
    /// `SIMCTL_CHILD_IDECRAFT_SELFTEST_*`. Debug builds only: a release build has no code that
    /// confirms digits by itself. `scripts/mobile-selftest.py` compares the digits with the host's.
    @MainActor
    enum SelfTest {
        static func runIfAsked(_ model: AppModel) async {
            let env = ProcessInfo.processInfo.environment
            guard let address = env["IDECRAFT_SELFTEST_ADDRESS"],
                let code = env["IDECRAFT_SELFTEST_CODE"],
                let folder = env["IDECRAFT_SELFTEST_FOLDER"],
                let file = env["IDECRAFT_SELFTEST_FILE"]
            else { return }
            func say(_ line: String) {
                print("SELFTEST \(line)")
                fflush(stdout)
            }
            await model.begin(address: address, secret: code)
            guard case .digits(let pairing) = model.screen else {
                say("FAIL pair \(model.error ?? "?")")
                return
            }
            say("digits=\(pairing.digits()) host=\(pairing.hostFingerprint())")
            await model.confirm(pairing)
            guard case .open(let paired) = model.screen else {
                say("FAIL confirm \(model.error ?? "?")")
                return
            }
            let vault = KeychainVault()
            let expected = kSecAttrAccessibleWhenUnlockedThisDeviceOnly as String
            for item in ["identity", "pins"] {
                let actual = vault.accessibility(of: item)
                say("keychain \(item) accessible=\(actual) expected=\(expected)")
            }
            say("paired address=\(paired) keychain_hosts=\(model.hosts.count)")
            await model.open(address: paired, folder: folder)
            guard case .files(let project) = model.screen else {
                say("FAIL open \(model.error ?? "?")")
                return
            }
            say("rights=\(project.rights().joined(separator: ","))")
            guard let entries = await model.run({ try project.list(path: ".") }) else {
                say("FAIL list \(model.error ?? "?")")
                return
            }
            say("entries=\(entries.map(\.name).joined(separator: ","))")
            guard let content = await model.run({ try project.readFile(path: file) }) else {
                say("FAIL read \(model.error ?? "?")")
                return
            }
            say("read path=\(content.path) size=\(content.size) first=\(content.text?.prefix(60) ?? "")")
            say("PASS")
        }
    }
#endif
