import Carbon
import Cocoa

/// The input-source database (Text Input Sources) as this input method sees it.
enum InputSources {
    private static func list(_ filter: [CFString: Any]) -> [TISInputSource] {
        (TISCreateInputSourceList(filter as CFDictionary, true)?.takeRetainedValue() as? [TISInputSource]) ?? []
    }

    /// The input method and its mode sources (all share the bundle identifier).
    static func all() -> [TISInputSource] {
        list([kTISPropertyBundleID: ModeID.base])
    }

    static func sources(id: String) -> [TISInputSource] {
        list([kTISPropertyInputSourceID: id])
    }

    static func flag(_ source: TISInputSource, _ key: CFString) -> Bool {
        guard let value = TISGetInputSourceProperty(source, key) else { return false }
        return CFBooleanGetValue(Unmanaged<CFBoolean>.fromOpaque(value).takeUnretainedValue())
    }

    /// Whether the mode's input source is enabled (added in System Settings), so selecting it can show its icon.
    static func isEnabled(_ id: String) -> Bool {
        sources(id: id).contains { flag($0, kTISPropertyInputSourceIsEnabled) }
    }

    static func appName(_ bundleID: String) -> String {
        if let app = NSRunningApplication.runningApplications(withBundleIdentifier: bundleID).first, let name = app.localizedName {
            return name
        }
        if let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: bundleID) {
            return FileManager.default.displayName(atPath: url.path).replacingOccurrences(of: ".app", with: "")
        }
        return bundleID
    }
}

/// macOS settings that take a switch key before the input method sees it (C-OPS-07 · M-03 · M-04). Read only; the user
/// changes them in System Settings.
struct SystemCheck {
    /// "Select the previous/next input source" is ⇧Space: the system switches input sources and ⇧Space never arrives.
    var shiftSpaceTaken = false
    /// "Use the Caps Lock key to switch to and from ABC" (global TISRomanSwitchState = 1) [I]: the system takes short presses.
    var capsLockSwitchesABC = false
    /// Mode input sources added in System Settings.
    var enabledModes: [InputMode] = []

    static func current() -> SystemCheck {
        var check = SystemCheck()
        let hotkeys = CFPreferencesCopyAppValue("AppleSymbolicHotKeys" as CFString, "com.apple.symbolichotkeys" as CFString)
            as? [String: Any] ?? [:]
        for key in ["60", "61"] {
            guard let entry = hotkeys[key] as? [String: Any], (entry["enabled"] as? Bool) ?? ((entry["enabled"] as? Int) == 1),
                  let value = entry["value"] as? [String: Any], let parameters = value["parameters"] as? [Int],
                  parameters.count == 3
            else { continue }
            // Parameters: character, key code, modifier mask. 49 = Space, 0x20000 = Shift alone.
            if parameters[1] == 49 && parameters[2] == 0x20000 { check.shiftSpaceTaken = true }
        }
        let roman = CFPreferencesCopyAppValue("TISRomanSwitchState" as CFString, kCFPreferencesAnyApplication)
        check.capsLockSwitchesABC = (roman as? Int) == 1
        check.enabledModes = ModeID.all.filter { InputSources.isEnabled(ModeID.id($0)) }
        return check
    }
}

enum SystemSettings {
    static func open(_ pane: String) {
        if let url = URL(string: "x-apple.systempreferences:" + pane) { NSWorkspace.shared.open(url) }
    }
    static let keyboard = "com.apple.Keyboard-Settings.extension"
    static let inputMonitoring = "com.apple.preference.security?Privacy_ListenEvent"
}
