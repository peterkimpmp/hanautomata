import Carbon
import Cocoa
import os

/// Process-wide state of the input method: the engine, the key logic, the active client and the learning file.
enum Shell {
    static let bundleID = Bundle.main.bundleIdentifier ?? ModeID.base
    static let log = Logger(subsystem: bundleID, category: "shell")
    static let latency = LatencyMeter()

    /// ~/Library/Application Support/Hanautomata/words.json — the Windows app's document format (C-OPS-09). This process
    /// is its only writer; the settings window runs here too.
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
        engine.setSwitchKind(kind: Settings.switchKind)
        engine.setLearningPaused(paused: Settings.learningPaused)
        _ = engine.setKeyboard(kind: Settings.keyboard)
        engine.setDeleteSyllable(enabled: Settings.deleteSyllable)
        engine.setMsClusters(enabled: Settings.msClusters)
        Lists.load(into: engine)
        let error = engine.storeError()
        if !error.isEmpty { log.error("learning file not loaded: \(error, privacy: .public)") }
        return engine
    }()

    static let core: InputCore = {
        let core = InputCore(engine: engine)
        core.feedback = CandidatePanel.shared
        core.save = saveSoon
        core.setCapsLock = CapsLock.set
        core.log = { message in log.error("\(message, privacy: .public)") }
        core.secureInput = { IsSecureEventInputEnabled() }
        core.checkField = { [unowned core] in FieldInspector.check { core.fieldChanged($0) } }
        return core
    }()

    /// Content-free counters next to the learning file (C-OPS-02), saved once a minute when they changed.
    static let diagnosticsPath = URL(fileURLWithPath: storePath).deletingLastPathComponent()
        .appendingPathComponent("diagnostics.json")

    static func startDiagnostics() {
        if let saved = Diagnostics.load(from: diagnosticsPath) { core.diagnostics.restore(saved) }
        Timer.scheduledTimer(withTimeInterval: 60, repeats: true) { _ in saveDiagnostics() }
    }

    static func saveDiagnostics() {
        guard core.diagnostics.dirty else { return }
        do { try core.diagnostics.save(to: diagnosticsPath) } catch {
            log.error("diagnostics not saved: \(error.localizedDescription, privacy: .public)")
        }
    }

    /// Pushes every setting to the engine and the monitors again (after an import).
    static func reapplySettings() {
        _ = engine.setMode(mode: Settings.mode)
        Settings.sensitivity = engine.setSensitivity(name: Settings.sensitivity)
        engine.setLearningEnabled(enabled: Settings.learning)
        engine.setSemicolonShortcut(enabled: Settings.semicolon)
        engine.setSwitchKind(kind: Settings.switchKind)
        _ = engine.setKeyboard(kind: Settings.keyboard)
        engine.setDeleteSyllable(enabled: Settings.deleteSyllable)
        engine.setMsClusters(enabled: Settings.msClusters)
        core.updateRecording()
        updateCapsMonitor()
        SourceLock.update()
    }

    /// The IMKTextInput object of the client being typed into, while this input method is active.
    private(set) static var activeSender: AnyObject?

    static var activeClient: TextClient? {
        activeSender.flatMap { IMKClient($0) }
    }

    static func activate(_ client: IMKClient) {
        activeSender = client.base
        KeyboardLayout.refresh()
        Lists.reloadIfChanged(engine)
        Updates.checkIfDue()
        updateCapsMonitor()
        core.activate(client, capsLocked: CapsLock.isOn())
        if !Settings.setupDone {
            Settings.setupDone = true
            DispatchQueue.main.async { SettingsWindow.show(firstRun: true) }
        }
    }

    static func deactivate(_ client: IMKClient?) {
        core.deactivate(client)
        saveDiagnostics()
        if let client, client.base === activeSender { activeSender = nil }
    }

    /// Caps Lock press/release need Input Monitoring; without it the core falls back to the lock changes InputMethodKit
    /// reports (Shift+Caps Lock locks, no long press).
    static func updateCapsMonitor() {
        let monitor = CapsLockMonitor.shared
        if Settings.capsLock && monitor.start() {
            monitor.onPress = { time, shift in core.capsPressed(at: time, shift: shift, client: activeClient) }
            monitor.onRelease = { time in core.capsReleased(at: time, client: activeClient) }
            core.capsMonitored = true
        } else {
            monitor.stop()
            core.capsMonitored = false
        }
    }

    private static let saveQueue = DispatchQueue(label: "\(bundleID).save", qos: .utility)

    /// Persists the dictionary off the key path.
    static func saveSoon() {
        saveQueue.async {
            let error = engine.save()
            if !error.isEmpty { log.error("learning file not saved: \(error, privacy: .public)") }
        }
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

/// The exception lists as text files the user can also edit (C-LEX-02): keep.txt (never convert) and convert.txt (always
/// convert) next to the learning file, one word per line, `#` comments. Edits apply the next time a field gets focus.
enum Lists {
    static let folder = URL(fileURLWithPath: Shell.storePath).deletingLastPathComponent()
    static let keepURL = folder.appendingPathComponent("keep.txt")
    static let convertURL = folder.appendingPathComponent("convert.txt")
    private static var stamp: [URL: Date] = [:]

    private static func read(_ url: URL, _ engine: Engine) -> [String] {
        stamp[url] = modified(url)
        return (try? String(contentsOf: url, encoding: .utf8)).map { engine.parseList(text: $0) } ?? []
    }

    private static func modified(_ url: URL) -> Date? {
        (try? FileManager.default.attributesOfItem(atPath: url.path))?[.modificationDate] as? Date
    }

    static func load(into engine: Engine) {
        engine.setExceptions(keep: read(keepURL, engine), convert: read(convertURL, engine))
    }

    static func reloadIfChanged(_ engine: Engine) {
        if modified(keepURL) != stamp[keepURL] || modified(convertURL) != stamp[convertURL] { load(into: engine) }
    }

    /// Applies new lists and writes both files.
    static func save(keep: [String], convert: [String], _ engine: Engine) {
        engine.setExceptions(keep: keep, convert: convert)
        let header = "# 한오토마타 예외 목록 — 한 줄에 한 단어, #으로 시작하면 설명\n"
        do {
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            try (header + "# 절대 바꾸지 않을 단어(영문 그대로)\n" + engine.keepList().joined(separator: "\n") + "\n")
                .write(to: keepURL, atomically: true, encoding: .utf8)
            try (header + "# 항상 한글로 바꿀 단어(한글 또는 영타로 적기)\n" + engine.convertList().joined(separator: "\n") + "\n")
                .write(to: convertURL, atomically: true, encoding: .utf8)
        } catch {
            Shell.log.error("exception lists not saved: \(error.localizedDescription, privacy: .public)")
        }
        stamp[keepURL] = modified(keepURL)
        stamp[convertURL] = modified(convertURL)
    }
}

/// Keeps Hanautomata selected when an app or a shortcut switches to another input source (M-05, off by default).
enum SourceLock {
    private static var observer: NSObjectProtocol?

    static func update() {
        if Settings.lockSource, observer == nil {
            observer = DistributedNotificationCenter.default().addObserver(
                forName: NSNotification.Name(kTISNotifySelectedKeyboardInputSourceChanged as String), object: nil,
                queue: .main) { _ in reselect() }
        } else if !Settings.lockSource, let current = observer {
            DistributedNotificationCenter.default().removeObserver(current)
            observer = nil
        }
    }

    private static func reselect() {
        guard Settings.lockSource, let current = TISCopyCurrentKeyboardInputSource()?.takeRetainedValue(),
              let pointer = TISGetInputSourceProperty(current, kTISPropertyInputSourceID)
        else { return }
        let id = Unmanaged<CFString>.fromOpaque(pointer).takeUnretainedValue() as String
        guard ModeID.mode(id) == nil else { return }
        let wanted = ModeID.id(Shell.engine.mode())
        let target = InputSources.isEnabled(wanted) ? wanted : ModeID.base
        if let source = InputSources.sources(id: target).first {
            TISSelectInputSource(source)
            Shell.log.info("input source lock: back from \(id, privacy: .public)")
        }
    }
}

/// Once a day, when the user turned it on, asks GitHub Releases for the newest version (C-OPS-04). Off means no network.
enum Updates {
    static let releases = URL(string: "https://github.com/peterkimpmp/hanautomata/releases/latest")!
    private static let api = URL(string: "https://api.github.com/repos/peterkimpmp/hanautomata/releases/latest")!

    static var current: String { Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "0" }

    /// Whether `latest` (a tag such as v1.2.0) is newer than `current`, comparing numeric parts.
    static func isNewer(_ latest: String, than current: String) -> Bool {
        func parts(_ v: String) -> [Int] {
            v.trimmingCharacters(in: CharacterSet(charactersIn: "vV")).split(separator: ".").map { Int($0.prefix { $0.isNumber }) ?? 0 }
        }
        let (a, b) = (parts(latest), parts(current))
        for i in 0..<max(a.count, b.count) {
            let (x, y) = (i < a.count ? a[i] : 0, i < b.count ? b[i] : 0)
            if x != y { return x > y }
        }
        return false
    }

    static func checkIfDue() {
        guard Settings.updateCheck else { return }
        if let last = Settings.lastUpdateCheck, Date().timeIntervalSince(last) < 86_400 { return }
        Settings.lastUpdateCheck = Date()
        var request = URLRequest(url: api, timeoutInterval: 10)
        request.setValue("application/vnd.github+json", forHTTPHeaderField: "Accept")
        URLSession.shared.dataTask(with: request) { data, _, _ in
            guard let data, let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                  let tag = json["tag_name"] as? String
            else { return }
            DispatchQueue.main.async { Settings.latestVersion = isNewer(tag, than: current) ? tag : "" }
        }.resume()
    }
}

/// What the about tab shows (C-OPS-10). The links open in the browser only when clicked.
enum About {
    static let repository = URL(string: "https://github.com/peterkimpmp/hanautomata")!
    static let issues = URL(string: "https://github.com/peterkimpmp/hanautomata/issues")!
    static let license = "MIT 라이선스 · © 2026 Taeyoung Kim (peterkimpmp)"

    static var build: String { Bundle.main.infoDictionary?["CFBundleVersion"] as? String ?? "?" }

    /// One line to paste into an issue: version, build, macOS version and architecture.
    static var summary: String {
        #if arch(arm64)
        let arch = "arm64"
        #else
        let arch = "x86_64"
        #endif
        let os = ProcessInfo.processInfo.operatingSystemVersion
        return "Hanautomata \(Updates.current) (\(build)) · macOS \(os.majorVersion).\(os.minorVersion).\(os.patchVersion) · \(arch)"
    }
}
