import Foundation

/// User settings (UserDefaults of the input-method bundle; the tests swap in their own suite). Read on every use, so a
/// change from the menu or the settings window applies without a restart (C-OPS-03).
enum Settings {
    static var defaults = UserDefaults.standard

    private static func bool(_ key: String, _ fallback: Bool) -> Bool {
        defaults.object(forKey: key) as? Bool ?? fallback
    }

    static var mode: InputMode {
        get {
            switch defaults.string(forKey: "mode") {
            case "korean": return .korean
            case "english": return .english
            default: return .auto
            }
        }
        set {
            let name: String
            switch newValue {
            case .auto: name = "auto"
            case .korean: name = "korean"
            case .english: name = "english"
            }
            defaults.set(name, forKey: "mode")
        }
    }

    static var sensitivity: String {
        get { defaults.string(forKey: "sensitivity") ?? "balanced" }
        set { defaults.set(newValue, forKey: "sensitivity") }
    }

    /// Switch keys (C-MOD-03 · M-03): ⇧Space and Caps Lock are required, right ⌘ tap is the macOS convention. All on by
    /// default, as the first-run setup recommends.
    static var shiftSpace: Bool {
        get { bool("shiftSpace", true) }
        set { defaults.set(newValue, forKey: "shiftSpace") }
    }

    static var capsLock: Bool {
        get { bool("capsLock", true) }
        set { defaults.set(newValue, forKey: "capsLock") }
    }

    static var rightCommand: Bool {
        get { bool("rightCommand", true) }
        set { defaults.set(newValue, forKey: "rightCommand") }
    }

    /// What a switch key does (C-MOD-02): single-shot (default), cycle or toggle.
    static var switchKind: SwitchKind {
        get {
            switch defaults.string(forKey: "switchKind") {
            case "cycle": return .cycle
            case "toggle": return .toggle
            default: return .singleShot
            }
        }
        set {
            let name: String
            switch newValue {
            case .singleShot: name = "single"
            case .cycle: name = "cycle"
            case .toggle: name = "toggle"
            }
            defaults.set(name, forKey: "switchKind")
        }
    }

    /// F12 toggles pass-through (C-MOD-04, the Windows app's key).
    static var pauseKey: Bool {
        get { bool("pauseKey", true) }
        set { defaults.set(newValue, forKey: "pauseKey") }
    }

    /// `;` after a word swaps the candidate (C-COR-01).
    static var semicolon: Bool {
        get { bool("semicolon", true) }
        set { defaults.set(newValue, forKey: "semicolon") }
    }

    static var learning: Bool {
        get { bool("learning", true) }
        set { defaults.set(newValue, forKey: "learning") }
    }

    /// Remembered choices still apply but no new ones are recorded (C-LEX-09).
    static var learningPaused: Bool {
        get { bool("learningPaused", false) }
        set { defaults.set(newValue, forKey: "learningPaused") }
    }

    /// Candidate window for every word with an alternative, not only uncertain ones (D12 shows it only when uncertain).
    static var candidateAlways: Bool {
        get { bool("candidateAlways", false) }
        set { defaults.set(newValue, forKey: "candidateAlways") }
    }

    /// Bundle identifiers where keys pass untouched (C-CTX-11).
    static var excludedApps: [String] {
        get { defaults.stringArray(forKey: "excludedApps") ?? [] }
        set { defaults.set(newValue, forKey: "excludedApps") }
    }

    // MARK: 1.0

    /// Korean layout (C-LAY-02): Dubeolsik (default), Sebeolsik 390 or Final.
    static var keyboard: KeyboardKind {
        get {
            switch defaults.string(forKey: "keyboard") {
            case "390": return .sebeolsik390
            case "final": return .sebeolsikFinal
            default: return .dubeolsik
            }
        }
        set {
            let name: String
            switch newValue {
            case .dubeolsik: name = "dubeolsik"
            case .sebeolsik390: name = "390"
            case .sebeolsikFinal: name = "final"
            }
            defaults.set(name, forKey: "keyboard")
        }
    }

    /// Backspace removes a whole syllable while composing (C-CMP-06).
    static var deleteSyllable: Bool {
        get { bool("deleteSyllable", false) }
        set { defaults.set(newValue, forKey: "deleteSyllable") }
    }

    /// MS Windows consonant clusters (C-CMP-08): ㄱ+ㅅ → ㄳ without a vowel.
    static var msClusters: Bool {
        get { bool("msClusters", false) }
        set { defaults.set(newValue, forKey: "msClusters") }
    }

    /// The language of the text before the caret breaks near-ties for the first word after a caret move (C-DET-08). Off by
    /// default: in held-out tests it adds 4 wrong conversions per 1,000 English words typed inside Korean text.
    static var caretContext: Bool {
        get { bool("caretContext", false) }
        set { defaults.set(newValue, forKey: "caretContext") }
    }

    /// In search fields the composition goes in as text, so the app searches as you type (C-CTX-07, experimental). Telling
    /// a search field needs accessibility access for Hanautomata, which the user gives in System Settings.
    static var searchInline: Bool {
        get { bool("searchInline", false) }
        set { defaults.set(newValue, forKey: "searchInline") }
    }

    /// Restore each app's last mode when it gets focus again (C-MOD-05).
    static var perAppMode: Bool {
        get { bool("perAppMode", true) }
        set { defaults.set(newValue, forKey: "perAppMode") }
    }

    static var appModes: [String: String] {
        get { defaults.dictionary(forKey: "appModes") as? [String: String] ?? [:] }
        set { defaults.set(newValue, forKey: "appModes") }
    }

    /// The user's app profiles (C-MOD-06), by bundle identifier.
    static var profiles: [String: AppProfile] {
        get {
            guard let data = defaults.data(forKey: "profiles") else { return [:] }
            return (try? JSONDecoder().decode([String: AppProfile].self, from: data)) ?? [:]
        }
        set { defaults.set(try? JSONEncoder().encode(newValue), forKey: "profiles") }
    }

    /// Built-in profiles for remote desktops, virtual machines (English start), terminals and code editors (conservative)
    /// (C-CTX-02 · C-CTX-03).
    static var builtInProfiles: Bool {
        get { bool("builtInProfiles", true) }
        set { defaults.set(newValue, forKey: "builtInProfiles") }
    }

    /// The profile that applies to an app: the user's, else a built-in one when those are on.
    static func profile(for app: String) -> AppProfile? {
        profiles[app] ?? (builtInProfiles ? AppProfile.builtIn[app] : nil)
    }

    /// Esc (and ⌃[) commits and switches to the English mode; the switch key returns (C-MOD-07 · M-09): off · all · apps.
    static var escVi: String {
        get { defaults.string(forKey: "escVi") ?? "off" }
        set { defaults.set(newValue, forKey: "escVi") }
    }

    static var viApps: [String] {
        get { defaults.stringArray(forKey: "viApps") ?? AppProfile.terminals }
        set { defaults.set(newValue, forKey: "viApps") }
    }

    /// Show the new mode next to the caret for a moment when it changes (C-MOD-08).
    static var modeNotice: Bool {
        get { bool("modeNotice", true) }
        set { defaults.set(newValue, forKey: "modeNotice") }
    }

    /// Candidate window text size (C-CMP-10): small · medium · large.
    static var candidateSize: String {
        get { defaults.string(forKey: "candidateSize") ?? "medium" }
        set { defaults.set(newValue, forKey: "candidateSize") }
    }

    /// Candidate window appearance (C-CMP-10): system · light · dark.
    static var candidateTheme: String {
        get { defaults.string(forKey: "candidateTheme") ?? "system" }
        set { defaults.set(newValue, forKey: "candidateTheme") }
    }

    /// Modifier taps that switch besides right ⌘ (C-MOD-03 · M-18): key codes of left/right Shift, right ⌥, right ⌃, fn.
    static var extraTaps: Set<UInt16> {
        get { Set((defaults.array(forKey: "extraTaps") as? [Int] ?? []).map { UInt16($0) }) }
        set { defaults.set(newValue.map { Int($0) }.sorted(), forKey: "extraTaps") }
    }

    /// Command keys (C-COR-07 · C-MOD-12 · custom switch key): missing = default, "" = unset.
    static var bindings: [Command: KeyBinding?] {
        get {
            let stored = defaults.dictionary(forKey: "bindings") as? [String: String] ?? [:]
            var result: [Command: KeyBinding?] = [:]
            for (name, value) in stored {
                guard let command = Command(rawValue: name) else { continue }
                result[command] = .some(KeyBinding(stored: value))
            }
            return result
        }
        set {
            var stored: [String: String] = [:]
            for (command, binding) in newValue { stored[command.rawValue] = binding?.stored ?? "" }
            defaults.set(stored, forKey: "bindings")
        }
    }

    static func binding(_ command: Command) -> KeyBinding? {
        bindings[command] ?? command.defaultBinding
    }

    /// ⌥ + key (M-06).
    static var optionKey: KeyMap.OptionKey {
        get { KeyMap.OptionKey(rawValue: defaults.string(forKey: "optionKey") ?? "") ?? .special }
        set { defaults.set(newValue.rawValue, forKey: "optionKey") }
    }

    /// The ` key types ₩ in the Korean mode (M-07).
    static var wonSign: Bool {
        get { bool("wonSign", false) }
        set { defaults.set(newValue, forKey: "wonSign") }
    }

    /// Keep Hanautomata selected when an app or a shortcut switches to another input source (M-05).
    static var lockSource: Bool {
        get { bool("lockSource", false) }
        set { defaults.set(newValue, forKey: "lockSource") }
    }

    /// Hanja candidates include KS X 1002 and name hanja (C-LEX-06).
    static var hanjaExtended: Bool {
        get { bool("hanjaExtended", false) }
        set { defaults.set(newValue, forKey: "hanjaExtended") }
    }

    /// Ask GitHub Releases once a day for a newer version (C-OPS-04); off means no network at all.
    static var updateCheck: Bool {
        get { bool("updateCheck", false) }
        set { defaults.set(newValue, forKey: "updateCheck") }
    }

    static var lastUpdateCheck: Date? {
        get { defaults.object(forKey: "lastUpdateCheck") as? Date }
        set { defaults.set(newValue, forKey: "lastUpdateCheck") }
    }

    /// The newest released version seen ("" = none newer).
    static var latestVersion: String {
        get { defaults.string(forKey: "latestVersion") ?? "" }
        set { defaults.set(newValue, forKey: "latestVersion") }
    }

    // MARK: Export and import (C-LEX-04)

    /// Settings that travel in an export bundle; machine-specific state (setup, update check times) stays.
    static let portable = [
        "mode", "sensitivity", "switchKind", "shiftSpace", "capsLock", "rightCommand", "pauseKey", "semicolon", "learning",
        "learningPaused", "candidateAlways", "excludedApps", "keyboard", "deleteSyllable", "msClusters", "perAppMode",
        "appModes", "builtInProfiles", "escVi", "viApps", "modeNotice", "candidateSize", "candidateTheme", "extraTaps",
        "bindings", "optionKey", "wonSign", "lockSource", "hanjaExtended", "updateCheck", "caretContext", "searchInline",
    ]

    /// The portable settings as a JSON object (profiles as an object, not encoded data).
    static func exported() -> String {
        var object: [String: Any] = [:]
        for key in portable {
            if let value = defaults.object(forKey: key) { object[key] = value }
        }
        if let data = defaults.data(forKey: "profiles"), let profiles = try? JSONSerialization.jsonObject(with: data) {
            object["profiles"] = profiles
        }
        let data = (try? JSONSerialization.data(withJSONObject: object, options: [.sortedKeys])) ?? Data("{}".utf8)
        return String(decoding: data, as: UTF8.self)
    }

    /// Applies the settings of an imported bundle that this version knows; returns how many were applied.
    @discardableResult
    static func imported(_ json: String) -> Int {
        guard let object = (try? JSONSerialization.jsonObject(with: Data(json.utf8))) as? [String: Any] else { return 0 }
        var applied = 0
        for key in portable {
            guard let value = object[key] else { continue }
            defaults.set(value, forKey: key)
            applied += 1
        }
        if let profiles = object["profiles"], let data = try? JSONSerialization.data(withJSONObject: profiles),
           (try? JSONDecoder().decode([String: AppProfile].self, from: data)) != nil {
            defaults.set(data, forKey: "profiles")
            applied += 1
        }
        return applied
    }

    /// The first-run setup was shown (C-OPS-07).
    static var setupDone: Bool {
        get { bool("setupDone", false) }
        set { defaults.set(newValue, forKey: "setupDone") }
    }
}

/// Per-app profile (C-MOD-06): the mode an app starts in, its sensitivity and strict protection. nil follows the global
/// settings.
struct AppProfile: Codable, Equatable {
    var mode: String?
    var sensitivity: String?
    var strict = false

    var inputMode: InputMode? { mode.flatMap(AppProfile.mode(named:)) }

    static func mode(named name: String) -> InputMode? {
        switch name {
        case "auto": return .auto
        case "korean": return .korean
        case "english": return .english
        default: return nil
        }
    }

    static func name(of mode: InputMode) -> String {
        switch mode {
        case .auto: return "auto"
        case .korean: return "korean"
        case .english: return "english"
        }
    }

    /// Remote desktops and virtual machines start in the English mode, so switch keys reach the remote system (C-CTX-02).
    static let remote = [
        "com.microsoft.rdc.macos", "com.microsoft.rdc.osx.beta", "com.vmware.fusion", "com.parallels.desktop.console",
        "com.utmapp.UTM", "com.citrix.receiver.icaviewer.mac", "com.realvnc.vncviewer", "com.teamviewer.TeamViewer",
        "com.p5sys.jump.mac.viewer", "com.apple.ScreenSharing", "org.virtualbox.app.VirtualBoxVM",
    ]
    /// Terminals and code editors start English-leaning: conservative sensitivity on top of the developer-token protection
    /// (C-CTX-03). Strict protection stays a per-app choice, so Korean typed in a terminal (chat, commit messages) still
    /// converts.
    static let terminals = [
        "com.apple.Terminal", "com.googlecode.iterm2", "dev.warp.Warp-Stable", "com.mitchellh.ghostty", "net.kovidgoyal.kitty",
        "io.alacritty", "com.github.wez.wezterm", "com.microsoft.VSCode", "com.microsoft.VSCodeInsiders",
        "com.todesktop.230313mzl4w4u92", "dev.zed.Zed", "com.sublimetext.4", "com.apple.dt.Xcode", "com.jetbrains.intellij",
        "com.jetbrains.intellij.ce", "com.jetbrains.pycharm", "com.jetbrains.pycharm.ce", "com.jetbrains.WebStorm",
        "com.jetbrains.goland", "com.jetbrains.CLion", "com.jetbrains.rider", "com.google.android.studio", "com.panic.Nova",
        "com.barebones.bbedit",
    ]

    static let builtIn: [String: AppProfile] = {
        var table: [String: AppProfile] = [:]
        for id in remote { table[id] = AppProfile(mode: "english", sensitivity: nil, strict: false) }
        for id in terminals { table[id] = AppProfile(mode: nil, sensitivity: "conservative", strict: false) }
        return table
    }()
}
