import Foundation
import os

/// Process-wide state of the input method: the engine, its settings and the learning file.
enum Shell {
    static let bundleID = Bundle.main.bundleIdentifier ?? "io.github.peterkimpmp.inputmethod.Hanautomata"
    static let log = Logger(subsystem: bundleID, category: "shell")
    static let latency = LatencyMeter()

    /// ~/Library/Application Support/Hanautomata/words.json — the Windows app's document format (C-OPS-09). This process
    /// is its only writer; the settings menu runs here too.
    static let storePath: String = {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? URL(fileURLWithPath: NSHomeDirectory()).appendingPathComponent("Library/Application Support")
        return base.appendingPathComponent("Hanautomata/words.json").path
    }()

    static let engine: Engine = {
        let engine = Engine(home: NSHomeDirectory(), storePath: storePath)
        _ = engine.setMode(mode: Settings.mode)
        Settings.sensitivity = engine.setSensitivity(name: Settings.sensitivity)
        engine.setLearningEnabled(enabled: Settings.learning)
        engine.setSemicolonShortcut(enabled: Settings.semicolon)
        let error = engine.storeError()
        if !error.isEmpty { log.error("learning file not loaded: \(error, privacy: .public)") }
        return engine
    }()

    private static let saveQueue = DispatchQueue(label: "\(bundleID).save", qos: .utility)

    /// Persists the dictionary off the key path.
    static func saveSoon() {
        saveQueue.async {
            let error = engine.save()
            if !error.isEmpty { log.error("learning file not saved: \(error, privacy: .public)") }
        }
    }
}

/// User settings (UserDefaults of the input-method bundle). Applied at launch and on every menu change.
enum Settings {
    private static let defaults = UserDefaults.standard

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

    static var shiftSpace: Bool {
        get { defaults.object(forKey: "shiftSpace") as? Bool ?? true }
        set { defaults.set(newValue, forKey: "shiftSpace") }
    }

    static var semicolon: Bool {
        get { defaults.object(forKey: "semicolon") as? Bool ?? true }
        set { defaults.set(newValue, forKey: "semicolon") }
    }

    static var learning: Bool {
        get { defaults.object(forKey: "learning") as? Bool ?? true }
        set { defaults.set(newValue, forKey: "learning") }
    }
}

/// Key-handling latency, from the shell receiving the key event to returning the composition and commit (C-OPS-05).
/// Counts only — no key or text is recorded. Logged every 1,000 keys.
final class LatencyMeter {
    private var samples: [UInt64] = []

    func record(nanoseconds: UInt64) {
        samples.append(nanoseconds)
        guard samples.count >= 1000 else { return }
        let sorted = samples.sorted()
        let p50 = Double(sorted[sorted.count / 2]) / 1e6
        let p99 = Double(sorted[sorted.count * 99 / 100]) / 1e6
        let max = Double(sorted[sorted.count - 1]) / 1e6
        Shell.log.info("key latency ms over \(sorted.count): p50 \(p50) p99 \(p99) max \(max)")
        samples.removeAll(keepingCapacity: true)
    }
}
