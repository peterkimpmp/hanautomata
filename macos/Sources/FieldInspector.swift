import ApplicationServices
import Foundation

/// What the focused text field is, read from the accessibility tree (C-CTX-07). Only when the user has given Hanautomata
/// accessibility access (System Settings > Privacy & Security > Accessibility); it is never requested from here. Reads
/// roles and identifiers only, never the field's text, and runs off the key path.
enum FieldInspector {
    private static let queue = DispatchQueue(label: "\(Shell.bundleID).field", qos: .userInitiated)
    private static var pending = false
    private static var last: TimeInterval = 0

    static var trusted: Bool { AXIsProcessTrusted() }

    /// Asks again (at most every 0.2 s, one at a time) and reports the answer on the main thread.
    static func check(_ done: @escaping (FieldKind) -> Void) {
        guard Settings.searchInline, !pending, trusted else { return }
        let now = ProcessInfo.processInfo.systemUptime
        guard now - last > 0.2 else { return }
        last = now
        pending = true
        queue.async {
            let kind = focusedKind()
            DispatchQueue.main.async {
                pending = false
                done(kind)
            }
        }
    }

    static func focusedKind() -> FieldKind {
        let a = focusedAttributes()
        return FieldKind.classify(role: a[kAXRoleAttribute] ?? "", subrole: a[kAXSubroleAttribute] ?? "",
                                  identifier: a[kAXIdentifierAttribute] ?? "")
    }

    /// Role, subrole, identifier and the app's own labels of the focused element (for `--inspect-field`, the C-CTX-07
    /// spike). A hung app costs at most 0.1 s per call.
    static func focusedAttributes() -> [String: String] {
        let system = AXUIElementCreateSystemWide()
        AXUIElementSetMessagingTimeout(system, 0.1)
        var focused: CFTypeRef?
        guard AXUIElementCopyAttributeValue(system, kAXFocusedUIElementAttribute as CFString, &focused) == .success,
              let focused, CFGetTypeID(focused) == AXUIElementGetTypeID() else { return [:] }
        let element = focused as! AXUIElement  // the type was checked above
        AXUIElementSetMessagingTimeout(element, 0.1)
        var out: [String: String] = [:]
        for name in [kAXRoleAttribute, kAXSubroleAttribute, kAXIdentifierAttribute, kAXRoleDescriptionAttribute,
                     kAXDescriptionAttribute, kAXPlaceholderValueAttribute] {
            var value: CFTypeRef?
            if AXUIElementCopyAttributeValue(element, name as CFString, &value) == .success, let text = value as? String {
                out[name] = text
            }
        }
        return out
    }
}
