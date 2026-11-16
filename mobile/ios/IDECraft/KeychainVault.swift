import Foundation
import Security

/// This device's key and its pinned hosts, in the iOS Keychain (ADR-0029 §4).
/// `WhenUnlockedThisDeviceOnly`: readable only while the device is unlocked, not in backups, not
/// migrated, not synchronized.
final class KeychainVault: SecretVault, @unchecked Sendable {
    private let service = "dev.idecraft.mobile"

    private func base(_ name: String) -> [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: name,
            kSecAttrSynchronizable as String: false,
        ]
    }

    private func failed(_ status: OSStatus) -> VaultError {
        .Failed(reason: "Keychain status \(status)")
    }

    func load(name: String) throws -> Data? {
        var query = base(name)
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var out: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &out)
        switch status {
        case errSecSuccess: return out as? Data
        case errSecItemNotFound: return nil
        default: throw failed(status)
        }
    }

    func save(name: String, value: Data) throws {
        let attributes: [String: Any] = [
            kSecValueData as String: value,
            kSecAttrAccessible as String: kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
        ]
        let updated = SecItemUpdate(base(name) as CFDictionary, attributes as CFDictionary)
        if updated == errSecSuccess { return }
        guard updated == errSecItemNotFound else { throw failed(updated) }
        let added = SecItemAdd(
            base(name).merging(attributes) { _, new in new } as CFDictionary, nil)
        guard added == errSecSuccess else { throw failed(added) }
    }

    #if DEBUG
        /// The accessibility class a stored item really has, for the self-test.
        func accessibility(of name: String) -> String {
            var query = base(name)
            query[kSecReturnAttributes as String] = true
            var out: CFTypeRef?
            guard SecItemCopyMatching(query as CFDictionary, &out) == errSecSuccess,
                let attributes = out as? [String: Any]
            else { return "none" }
            return attributes[kSecAttrAccessible as String] as? String ?? "none"
        }
    #endif

    func delete(name: String) throws {
        let status = SecItemDelete(base(name) as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else { throw failed(status) }
    }
}
