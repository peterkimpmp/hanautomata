import Foundation

// The input method's key logic without InputMethodKit: the IMK controller adapts NSEvent and IMKTextInput to this, and
// Tests/Input drives it with a text buffer. One instance per process; only one client is active at a time and its
// composition is committed whenever focus leaves it.

/// A text field the input method edits (InputMethodKit's client in the app, a buffer in the tests).
protocol TextClient: AnyObject {
    var bundleIdentifier: String { get }
    /// Stable across event adapters for the same underlying input client.
    var identity: ObjectIdentifier { get }
    /// Inserts at the composition or selection (`nil`), or replaces `range`; ends the composition.
    func insert(_ text: String, replacing range: NSRange?)
    /// Shows the composition at the caret; empty clears it.
    func setMarked(_ text: String, uncertain: Bool)
    /// nil when the client cannot tell.
    func selectedRange() -> NSRange?
    /// nil when the client cannot read the range.
    func text(in range: NSRange) -> String?
    /// The caret's line in screen coordinates, for the window near it.
    func caretRect() -> CGRect?
    /// Shows the mode's input source in the menu bar; false when that source is not enabled.
    func selectMode(_ id: String) -> Bool
}

extension TextClient {
    var identity: ObjectIdentifier { ObjectIdentifier(self) }
}

/// Where the caret is, asked only when the window is drawn: the client call it needs can be slow, so it stays off the key
/// path (C-OPS-05).
typealias Caret = () -> CGRect?

/// The small window near the caret: a candidate with its evidence (C-CMP-02) or a short notice.
protocol Feedback: AnyObject {
    func showCandidate(_ text: String, hint: String, near rect: @escaping Caret)
    /// `sticky` stays until the next key; otherwise it fades by itself.
    func showNotice(_ text: String, near rect: @escaping Caret, sticky: Bool)
    /// The new mode next to the caret for about a second, without taking input (C-MOD-08).
    func showMode(_ text: String, near rect: @escaping Caret)
    /// Numbered choices (hanja, C-LEX-06): the current page of `items`, `selected` highlighted.
    func showChoices(_ items: [String], selected: Int, title: String, near rect: @escaping Caret)
    func hide()
}

struct Modifiers: OptionSet {
    let rawValue: Int
    static let shift = Modifiers(rawValue: 1 << 0)
    static let capsLock = Modifiers(rawValue: 1 << 1)
    static let control = Modifiers(rawValue: 1 << 2)
    static let option = Modifiers(rawValue: 1 << 3)
    static let command = Modifiers(rawValue: 1 << 4)
    static let leftCommand = Modifiers(rawValue: 1 << 5)
    static let rightCommand = Modifiers(rawValue: 1 << 6)
    static let function = Modifiers(rawValue: 1 << 7)
    static let leftShift = Modifiers(rawValue: 1 << 8)
    static let rightShift = Modifiers(rawValue: 1 << 9)
    static let rightOption = Modifiers(rawValue: 1 << 10)
    static let rightControl = Modifiers(rawValue: 1 << 11)
}

extension TapKey {
    /// The side-specific flag that says this key is held.
    var held: Modifiers {
        switch self {
        case .leftShift: return .leftShift
        case .rightShift: return .rightShift
        case .rightOption: return .rightOption
        case .rightControl: return .rightControl
        case .rightCommand: return .rightCommand
        case .function: return .function
        }
    }

    /// Flags that mean another modifier is held too (then it is a chord, not a tap).
    var others: Modifiers {
        switch self {
        case .leftShift: return [.rightShift, .control, .option, .command]
        case .rightShift: return [.leftShift, .control, .option, .command]
        case .rightOption: return [.shift, .control, .command]
        case .rightControl: return [.shift, .option, .command]
        case .rightCommand: return [.shift, .control, .option, .leftCommand]
        case .function: return [.shift, .control, .option, .command]
        }
    }
}

func modeName(_ mode: InputMode) -> String {
    switch mode {
    case .auto: return "자동 판별"
    case .korean: return "한글 고정"
    case .english: return "영문 고정"
    }
}

/// Short mode label next to the caret (C-MOD-08), as the menu-bar icons read.
func modeBadge(_ mode: InputMode) -> String {
    switch mode {
    case .auto: return "가A 자동"
    case .korean: return "가 한글"
    case .english: return "A 영문"
    }
}

/// Input modes as input sources (C-MOD-08): each mode has its own menu-bar icon. The automatic mode keeps the bundle's
/// identifier, so an input source added before the modes existed stays enabled.
enum ModeID {
    static let base = "io.github.peterkimpmp.inputmethod.Hanautomata"
    static let all: [InputMode] = [.auto, .korean, .english]

    static func id(_ mode: InputMode) -> String {
        switch mode {
        case .auto: return base
        case .korean: return base + ".Korean"
        case .english: return base + ".English"
        }
    }

    static func mode(_ id: String) -> InputMode? {
        all.first { self.id($0) == id }
    }
}

/// What the focused text field is, when the accessibility tree tells (C-CTX-07). InputMethodKit itself says nothing about
/// the field; URL, e-mail and number fields are not told apart on macOS (C-DET-07; Windows reads the input scope).
enum FieldKind: Equatable {
    case standard, search

    /// Search fields: the AXSearchField subrole (NSSearchField, HTML type=search) and Safari's address-and-search field
    /// (its identifier as Safari reports it [U]: confirm with `Hanautomata --inspect-field`).
    static func classify(role: String, subrole: String, identifier: String) -> FieldKind {
        if subrole == "AXSearchField" { return .search }
        if identifier == "WEB_BROWSER_ADDRESS_AND_SEARCH_FIELD" { return .search }
        return .standard
    }
}

enum KeyCode {
    static let capsLock: UInt16 = 57
    static let rightCommand: UInt16 = 54
    static let f12: UInt16 = 111
}

final class InputCore {
    let engine: Engine
    weak var feedback: Feedback?
    /// Persist the learning file off the key path.
    var save: () -> Void = {}
    /// Set the Caps Lock state (and its light).
    var setCapsLock: (Bool) -> Void = { _ in }
    var log: (String) -> Void = { _ in }
    /// A secure (password) field has focus somewhere; nothing typed may be learned (C-CTX-01).
    var secureInput: () -> Bool = { false }
    /// Asks, off the key path, what the focused field is; the answer comes back through `fieldChanged` (C-CTX-07).
    var checkField: () -> Void = {}
    private(set) var field = FieldKind.standard
    let diagnostics = Diagnostics()
    /// The text committed last outside a live key (focus left, the client went away, the engine failed): the input
    /// method keeps it in memory only, so it can be inserted again (C-CMP-11).
    private(set) var kept = ""
    /// The Caps Lock key's press and release reach `capsPressed`/`capsReleased` (Input Monitoring granted); the lock
    /// changes InputMethodKit reports are then ignored.
    var capsMonitored = false

    private var shiftSpace = ShiftSpaceDetector()
    private var taps = Dictionary(uniqueKeysWithValues: TapKey.allCases.map { ($0, ModifierTap()) })
    private(set) var caps = CapsLockGesture()
    private var hasMarked = false
    private var currentMode: InputMode
    /// The mode input source the system last reported or we selected.
    private var reportedMode: String?
    /// The decision behind the word on screen, recorded when it commits.
    private var shown: (confidence: String, korean: Bool)?
    /// Recording is off because of a secure field or the learning pause.
    private var recordingOff = false
    /// The app of the active client, for per-app modes and profiles.
    private(set) var activeApp = ""
    private(set) var proofreadingEpoch: UInt64 = 0
    var proofreadingAllowed: Bool { !secureInput() && !engine.paused() && !engine.isComposing() && !hasMarked && choice == nil }
    /// Esc switched to the English mode (vi mode); the switch key returns to this mode (C-MOD-07).
    private(set) var viReturn: InputMode?
    /// An app activation is setting its remembered or start mode: not a choice to save as the user's mode.
    private var restoringMode = false
    /// vi mode's English (Esc): remembered for the app, but not the user's mode for other apps.
    private var appOnlyMode = false

    /// Hanja candidates being chosen for one syllable (C-LEX-06 · M-08).
    private struct Choice {
        var range: NSRange
        var syllable: String
        var items: [String]
        var index = 0
        static let page = 9
    }
    private var choice: Choice?

    /// C-CTX-07: in a search field the composition goes in as plain text, because apps search only what is committed.
    /// Where that text is and what it says, so the next change replaces exactly it.
    private var inline: (range: NSRange, text: String)?

    private struct SpacedCommit {
        let client: ObjectIdentifier
        let epoch: UInt64
        let range: NSRange
        let original: String
        let corrected: String
        let word: String
        let time: TimeInterval
    }
    private var spacedCommit: SpacedCommit?
    // A rejected correction stays off for this process only; bounded and never saved as a language preference.
    private var unspacedWords = Set<String>()
    private var unspacedOrder: [String] = []
    var spacingClock: () -> TimeInterval = { ProcessInfo.processInfo.systemUptime }

    init(engine: Engine) {
        self.engine = engine
        currentMode = engine.mode()
    }

    // MARK: Client lifecycle

    func activate(_ client: TextClient, capsLocked: Bool) {
        proofreadingEpoch &+= 1
        spacedCommit = nil
        engine.forgetReconversion()
        shiftSpace.reset()
        resetTaps()
        caps.sync(locked: capsLocked)
        hasMarked = false
        inline = nil
        field = .standard
        checkField()
        choice = nil
        let app = client.bundleIdentifier
        let profile = Settings.profile(for: app)
        _ = engine.setSensitivity(name: profile?.sensitivity ?? Settings.sensitivity)
        engine.setStrict(strict: profile?.strict ?? false)
        if app != activeApp {
            activeApp = app
            viReturn = nil
            // C-MOD-05 · C-MOD-06: the app's last mode, else its profile's start mode, else the mode the user last chose.
            // Only the last is global: a remote desktop's English start must not follow the user into the next app.
            let saved = Settings.perAppMode ? Settings.appModes[app].flatMap(AppProfile.mode(named:)) : nil
            let mode = saved ?? profile?.inputMode ?? Settings.mode
            if mode != engine.mode() {
                restoringMode = true
                apply(engine.setMode(mode: mode), to: client, announce: false)
                restoringMode = false
            }
        }
    }

    func deactivate(_ client: TextClient?) {
        proofreadingEpoch &+= 1
        commitPending(client)
        choice = nil
        feedback?.hide()
        shiftSpace.reset()
        resetTaps()
    }

    /// The answer to `checkField`. A word already started keeps the way it is shown until it ends.
    func fieldChanged(_ kind: FieldKind) {
        field = kind
    }

    private func resetTaps() {
        for key in TapKey.allCases { taps[key]?.reset() }
    }

    private func enabledTaps() -> Set<UInt16> {
        var keys = Settings.extraTaps
        if Settings.rightCommand { keys.insert(TapKey.rightCommand.rawValue) }
        return keys
    }

    /// Focus left the composition: a click, the system ending it, or deactivation.
    func commitPending(_ client: TextClient?) {
        spacedCommit = nil
        let result = engine.commitPending()
        if !result.commit.isEmpty { kept = result.commit }
        apply(result, to: client, path: "focus")
    }

    /// Inserts the kept text at the caret (input menu), once.
    func reinsertKept(_ client: TextClient?) {
        guard let client, !kept.isEmpty else { return }
        let text = kept
        kept = ""
        commitPending(client)
        client.insert(text, replacing: nil)
    }

    func isExcluded(_ client: TextClient) -> Bool {
        let id = client.bundleIdentifier
        return !id.isEmpty && Settings.excludedApps.contains(id)
    }

    /// Adds or removes the client's app from the exclusion list (C-CTX-11), committing first.
    func toggleExclusion(_ client: TextClient) {
        let id = client.bundleIdentifier
        guard !id.isEmpty else { return }
        var apps = Settings.excludedApps
        if let index = apps.firstIndex(of: id) {
            apps.remove(at: index)
        } else {
            commitPending(client)
            apps.append(id)
        }
        Settings.excludedApps = apps
    }

    // MARK: Keys

    /// Returns whether the key is consumed. `injected`: the event carries text another program inserted as Unicode, not
    /// what the keyboard produces for the key (C-CTX-08).
    func keyDown(keyCode: UInt16, modifiers: Modifiers, isRepeat: Bool, injected: Bool = false,
                 client: TextClient) -> Bool {
        for key in TapKey.allCases { taps[key]?.keyDown() }
        let app = client.bundleIdentifier
        diagnostics.key(app: app)
        let excluded = isExcluded(client)
        let (shift, control, option, command) = (modifiers.contains(.shift), modifiers.contains(.control),
                                                 modifiers.contains(.option), modifiers.contains(.command))
        let bound = KeyMap.command(keyCode: keyCode, shift: shift, control: control, option: option, command: command,
                                   bindings: Settings.bindings)
        if bound != .candidate { spacedCommit = nil }
        if secureInput() || excluded || engine.paused() {
            spacedCommit = nil
            engine.forgetReconversion()
        }
        if bound == .pause, Settings.pauseKey, !excluded {
            if !isRepeat {
                setPaused(!engine.paused(), client: client)
                feedback?.showNotice(engine.paused() ? "일시정지 — 그대로 입력합니다 (F12로 다시 켜기)" : "다시 판별합니다",
                                     near: { client.caretRect() }, sticky: false)
            }
            return true
        }
        if excluded || engine.paused() {
            _ = shiftSpace.keyDown(isSpace: keyCode == KeyMap.space)
            diagnostics.passed(excluded ? "excluded" : "paused", app: app)
            return false
        }
        if injected {
            // Unicode text from another program (a typing assistant, a text expander) goes in as it is, after the word
            // being composed.
            _ = shiftSpace.keyDown(isSpace: false)
            commitPending(client)
            diagnostics.passed("injected", app: app)
            return false
        }
        if choice != nil, let handled = choose(keyCode: keyCode, modifiers: modifiers, client) {
            return handled
        }
        updateRecording()
        switch bound {
        case .candidate:
            _ = shiftSpace.keyDown(isSpace: false)
            if isRepeat { return true }
            if !engine.isComposing(), let spaced = spacedCommit {
                restoreSpacing(spaced, client)
                return true
            }
            if !engine.isComposing(), let swap = engine.reconversion() {
                reconvert(swap, client)
                return true
            }
            if engine.isComposing() { diagnostics.correction("candidate") }
            return run(.candidate, client)
        case .flip:
            _ = shiftSpace.keyDown(isSpace: false)
            if engine.isComposing() {
                diagnostics.correction("candidate")
                return run(.candidate, client)
            }
            if !isRepeat { flipSelection(client) }
            return true
        case .recent:
            _ = shiftSpace.keyDown(isSpace: false)
            if !isRepeat { flipRecent(client) }
            return true
        case .hanja:
            _ = shiftSpace.keyDown(isSpace: false)
            if isRepeat || startHanja(client) { return true }
        case .switchKey:
            _ = shiftSpace.keyDown(isSpace: false)
            if !isRepeat { _ = switchLanguage(client) }
            return true
        case .modeAuto, .modeKorean, .modeEnglish:
            _ = shiftSpace.keyDown(isSpace: false)
            if !isRepeat { setMode(bound == .modeAuto ? .auto : bound == .modeKorean ? .korean : .english, client: client) }
            return true
        case .pause, nil:
            break
        }
        // Esc / ⌃[ in vi mode (C-MOD-07 · M-09): commit, switch to English, and let the app have the key.
        if (keyCode == 53 && !shift && !control && !option && !command) || (keyCode == 33 && control && !option && !command),
           viApplies(app) {
            _ = shiftSpace.keyDown(isSpace: false)
            if engine.mode() != .english {
                viReturn = engine.mode()
                appOnlyMode = true
                apply(engine.setMode(mode: .english), to: client, path: "escape")
                appOnlyMode = false
            }
            return false
        }
        // ` → ₩ in the Korean mode on Dubeolsik (M-07).
        if keyCode == 50, Settings.wonSign, !shift, !control, !option, !command, engine.mode() == .korean,
           Settings.keyboard == .dubeolsik {
            _ = shiftSpace.keyDown(isSpace: false)
            _ = run(.other, client)
            client.insert("₩", replacing: nil)
            return true
        }
        let action = KeyMap.action(keyCode: keyCode, shift: shift, capsLock: modifiers.contains(.capsLock),
                                   command: command, control: control, option: option, isRepeat: isRepeat,
                                   shiftSpaceEnabled: Settings.shiftSpace, optionKey: Settings.optionKey,
                                   detector: &shiftSpace)
        switch action {
        case .ignore: return true
        case .text(let raw, let key):
            if Settings.caretContext, !engine.isComposing() { readCaretText(client) }
            return run(.text, raw: String(raw), key: String(key), client)
        case .space: return run(.space, client)
        case .switchKey:
            return switchLanguage(client)
        case .english(let c):
            _ = run(.shortcut, client)
            client.insert(String(c), replacing: nil)
            return true
        case .backspace: return run(.backspace, client)
        case .escape: return run(.escape, client)
        case .enter: return run(.enter, client)
        case .tab: return run(.tab, client)
        case .navigation: return run(.navigation, client)
        case .shortcut: return run(.shortcut, client)
        case .other: return run(.other, client)
        }
    }

    /// Modifier changes: arms ⇧Space, recognises modifier taps (right ⌘ and the optional ones) and, without the monitor,
    /// Caps Lock. Never consumed.
    func flagsChanged(keyCode: UInt16, modifiers: Modifiers, time: TimeInterval, client: TextClient) -> Bool {
        shiftSpace.flagsChanged(shift: modifiers.contains(.shift))
        if isExcluded(client) || engine.paused() {
            resetTaps()
            return false
        }
        let enabled = enabledTaps()
        var tapped = false
        for key in TapKey.allCases {
            let others = !modifiers.intersection(key.others).isEmpty
            if taps[key]?.changed(isTapKey: keyCode == key.rawValue, down: modifiers.contains(key.held), others: others,
                                  at: time) == true, enabled.contains(key.rawValue) {
                tapped = true
            }
        }
        if tapped { _ = switchLanguage(client) }
        if keyCode == KeyCode.capsLock, Settings.capsLock, !capsMonitored {
            apply(caps.lockChanged(on: modifiers.contains(.capsLock), shift: modifiers.contains(.shift)), client)
        }
        return false
    }

    /// Caps Lock press seen by the monitor; ignored unless this input method is active for `client`.
    func capsPressed(at time: TimeInterval, shift: Bool, client: TextClient?) {
        guard let client, Settings.capsLock, !engine.paused(), !isExcluded(client) else { return }
        caps.press(at: time, shift: shift)
    }

    func capsReleased(at time: TimeInterval, client: TextClient?) {
        apply(caps.release(at: time), client)
    }

    private func apply(_ outcome: CapsLockGesture.Outcome, _ client: TextClient?) {
        switch outcome {
        case .none: break
        case .switchLanguage:
            setCapsLock(false)
            if let client { _ = switchLanguage(client) }
        case .lock: setCapsLock(true)
        case .unlock: setCapsLock(false)
        }
    }

    /// Every switch key (⇧Space, taps, Caps Lock, a custom key) lands here: back from vi mode's English, or the engine's
    /// switch meaning.
    private func switchLanguage(_ client: TextClient) -> Bool {
        diagnostics.correction("switch")
        if let back = viReturn {
            viReturn = nil
            if engine.mode() == .english {
                apply(engine.setMode(mode: back), to: client, path: "switch")
                return true
            }
        }
        return run(.switch, client)
    }

    /// C-DET-08: the 40 characters before the caret, for the engine's tie-break on the word about to start.
    private func readCaretText(_ client: TextClient) {
        guard let selection = client.selectedRange(), selection.location > 0 else { return }
        let start = max(0, selection.location - 40)
        if let text = client.text(in: NSRange(location: start, length: selection.location - start)) {
            engine.setCaretText(text: text)
        }
    }

    /// A command from the input menu (the same as its key).
    func run(command: Command, client: TextClient) {
        switch command {
        case .flip: flipSelection(client)
        case .recent: flipRecent(client)
        case .hanja: _ = startHanja(client)
        default: break
        }
    }

    private func viApplies(_ app: String) -> Bool {
        switch Settings.escVi {
        case "all": return true
        case "apps": return Settings.viApps.contains(app)
        default: return false
        }
    }

    private func run(_ kind: KeyKind, raw: String = "", key: String = "", _ client: TextClient) -> Bool {
        let result = engine.handle(kind: kind, raw: raw, key: key)
        apply(result, to: client, path: InputCore.path(kind))
        return result.handled
    }

    /// What committed a word, for the diagnostics.
    private static func path(_ kind: KeyKind) -> String {
        switch kind {
        case .space: return "space"
        case .enter: return "enter"
        case .tab: return "tab"
        case .navigation: return "navigation"
        case .shortcut: return "shortcut"
        case .switch: return "switch"
        case .text: return "bound"
        default: return "other"
        }
    }

    /// Nothing typed in a secure field, or while learning is paused, is recorded (C-CTX-01 · C-LEX-09).
    func updateRecording() {
        let secure = secureInput()
        if secure { diagnostics.secureKey() }
        let off = secure || Settings.learningPaused
        guard off != recordingOff || engine.learningPaused() != off else { return }
        recordingOff = off
        engine.setLearningPaused(paused: off)
    }

    // MARK: Modes

    /// Mode chosen in the input menu, the settings window or a mode shortcut.
    func setMode(_ mode: InputMode, client: TextClient?) {
        spacedCommit = nil
        viReturn = nil
        apply(engine.setMode(mode: mode), to: client)
    }

    /// Layout chosen in the settings (C-LAY-02); the word in progress is committed first.
    func setKeyboard(_ kind: KeyboardKind, client: TextClient?) {
        spacedCommit = nil
        apply(engine.setKeyboard(kind: kind), to: client, announce: false)
    }

    func setPaused(_ paused: Bool, client: TextClient?) {
        spacedCommit = nil
        apply(engine.setPaused(paused: paused), to: client)
    }

    /// The system selected one of our mode input sources (menu bar, ⌃Space), or reports the current one when a client
    /// activates. Reports that repeat the last known source are ignored, so a mode chosen in our own menu survives app
    /// switches when its source is not enabled. At launch only a fixed-mode source overrides the saved mode.
    func systemSelected(modeID id: String, client: TextClient?) {
        guard let mode = ModeID.mode(id) else { return }
        let first = reportedMode == nil
        guard id != reportedMode else { return }
        reportedMode = id
        if first && mode == .auto { return }
        guard mode != engine.mode() else { return }
        currentMode = mode
        Settings.mode = mode
        viReturn = nil
        rememberAppMode(mode)
        apply(engine.setMode(mode: mode), to: client, announce: false)
    }

    private func rememberAppMode(_ mode: InputMode) {
        guard Settings.perAppMode, !activeApp.isEmpty else { return }
        var modes = Settings.appModes
        modes[activeApp] = AppProfile.name(of: mode)
        Settings.appModes = modes
    }

    /// Keeps the menu-bar mode icon, the saved setting and the app's remembered mode in step with the engine (fixed modes
    /// toggle on a switch key); `announce` shows the new mode next to the caret.
    private func syncMode(_ client: TextClient?, announce: Bool) {
        let mode = engine.mode()
        guard mode != currentMode else { return }
        currentMode = mode
        if !restoringMode {
            if !appOnlyMode { Settings.mode = mode }
            rememberAppMode(mode)
        }
        let id = ModeID.id(mode)
        if let client, client.selectMode(id) { reportedMode = id }
        if announce, Settings.modeNotice, let client, !engine.isComposing() {
            feedback?.showMode(modeBadge(mode), near: { client.caretRect() })
        }
    }

    // MARK: Correction (C-COR-02 · C-COR-03)

    private func spacingText(_ text: String, _ client: TextClient) -> String {
        let word = text.hasSuffix(" ") ? String(text.dropLast()) : text
        guard Settings.autoSpacing, !secureInput(), !isExcluded(client), !unspacedWords.contains(word) else { return text }
        return engine.correctSpacing(text: text)
    }

    private func rememberSpacing(original: String, corrected: String, _ client: TextClient) {
        let length = corrected.utf16.count
        guard let caret = client.selectedRange(), caret.location != NSNotFound, caret.length == 0,
              caret.location >= length else { return }
        let range = NSRange(location: caret.location - length, length: length)
        guard client.text(in: range) == corrected else { return }
        spacedCommit = SpacedCommit(client: client.identity, epoch: proofreadingEpoch, range: range,
                                    original: original, corrected: corrected,
                                    word: String(original.dropLast()), time: spacingClock())
    }

    /// First F2 restores spacing only. A subsequent F2 can still use the engine's original language alternative.
    private func restoreSpacing(_ spaced: SpacedCommit, _ client: TextClient) {
        spacedCommit = nil
        let age = spacingClock() - spaced.time
        guard !secureInput(), client.identity == spaced.client, proofreadingEpoch == spaced.epoch,
              age >= 0, age <= 15,
              client.selectedRange() == NSRange(location: NSMaxRange(spaced.range), length: 0),
              client.text(in: spaced.range) == spaced.corrected else { return failReconversion(client) }
        client.insert(spaced.original, replacing: spaced.range)
        if unspacedWords.insert(spaced.word).inserted {
            unspacedOrder.append(spaced.word)
            if unspacedOrder.count > 256 { unspacedWords.remove(unspacedOrder.removeFirst()) }
        }
        diagnostics.correction("spacingUndo")
        feedback?.hide()
    }

    /// F2 right after a word committed with Space: if the word is still right before the caret, swap it for the other
    /// candidate and remember the choice; otherwise change nothing and say so.
    private func reconvert(_ swap: Reconversion, _ client: TextClient) {
        let length = swap.old.utf16.count
        guard let selection = client.selectedRange(), selection.location != NSNotFound, selection.length == 0,
              selection.location >= length
        else { return failReconversion(client) }
        let range = NSRange(location: selection.location - length, length: length)
        guard client.text(in: range) == swap.old else { return failReconversion(client) }
        client.insert(swap.new, replacing: range)
        let result = engine.reconvert()
        if result.learned { save() }
        diagnostics.correction("reconvert")
        feedback?.hide()
    }

    private func failReconversion(_ client: TextClient) {
        engine.forgetReconversion()
        diagnostics.correction("reconvertFailed")
        feedback?.showNotice("직전 단어를 찾지 못해 바꾸지 않았습니다", near: { client.caretRect() }, sticky: false)
    }

    /// Shift+F2: flip the selection between Hangul and QWERTY keys in place, without the clipboard. The app's undo
    /// reverts it.
    private func flipSelection(_ client: TextClient) {
        guard let selection = client.selectedRange(), selection.location != NSNotFound else {
            diagnostics.correction("flipFailed")
            return notice("선택한 글자를 읽지 못했습니다", client)
        }
        guard selection.length > 0 else { return notice("바꿀 글자를 먼저 선택하세요", client) }
        guard let text = client.text(in: selection), text.utf16.count == selection.length else {
            diagnostics.correction("flipFailed")
            return notice("선택한 글자를 읽지 못했습니다", client)
        }
        let flipped = engine.flip(text: text)
        if flipped != text { client.insert(flipped, replacing: selection) }
        diagnostics.correction("flip")
        feedback?.hide()
    }

    private func notice(_ text: String, _ client: TextClient) {
        feedback?.showNotice(text, near: { client.caretRect() }, sticky: false)
    }

    /// The last three words before the caret flip between Hangul and QWERTY keys at once (C-COR-04), within the line.
    /// No learning; the app's undo reverts it.
    private func flipRecent(_ client: TextClient, words: Int = 3) {
        if engine.isComposing() { apply(engine.commitPending(), to: client, path: "recent") }
        guard let selection = client.selectedRange(), selection.length == 0, selection.location > 0 else {
            diagnostics.correction("recentFailed")
            return notice("커서 앞 글자를 읽지 못했습니다", client)
        }
        let start = max(0, selection.location - 200)
        guard let text = client.text(in: NSRange(location: start, length: selection.location - start)) else {
            diagnostics.correction("recentFailed")
            return notice("커서 앞 글자를 읽지 못했습니다", client)
        }
        let chars = Array(text)
        var i = chars.count
        var found = 0
        while i > 0 && found < words {
            while i > 0 && (chars[i - 1] == " " || chars[i - 1] == "\t") { i -= 1 }
            guard i > 0, !chars[i - 1].isNewline else { break }
            while i > 0 && !chars[i - 1].isWhitespace { i -= 1 }
            found += 1
        }
        let suffix = String(chars[i...])
        let flipped = engine.flip(text: suffix)
        guard found > 0, flipped != suffix else { return notice("바꿀 단어가 없습니다", client) }
        let length = suffix.utf16.count
        client.insert(flipped, replacing: NSRange(location: selection.location - length, length: length))
        engine.forgetReconversion()
        diagnostics.correction("recent")
        feedback?.hide()
    }

    // MARK: Hanja (C-LEX-06 · M-08)

    /// ⌥↩: hanja for the selected syllable, else the syllable before the caret (the word being composed is committed
    /// first). Returns false when there is nothing to convert, so the key reaches the app.
    private func startHanja(_ client: TextClient) -> Bool {
        if engine.isComposing() { apply(engine.commitPending(), to: client, path: "hanja") }
        guard let selection = client.selectedRange(), selection.location != NSNotFound else { return false }
        let range = selection.length > 0 ? selection : NSRange(location: selection.location - 1, length: 1)
        guard range.location >= 0, range.length == 1, let text = client.text(in: range),
              let scalar = text.unicodeScalars.first, (0xAC00...0xD7A3).contains(scalar.value)
        else { return false }
        let items = engine.hanja(syllable: text, extended: Settings.hanjaExtended)
        guard !items.isEmpty else {
            notice("‘\(text)’에 해당하는 한자가 없습니다", client)
            return true
        }
        choice = Choice(range: range, syllable: text, items: items)
        showChoice(client)
        return true
    }

    private func showChoice(_ client: TextClient) {
        guard let choice else { return }
        let first = choice.index / Choice.page * Choice.page
        let page = Array(choice.items[first..<min(first + Choice.page, choice.items.count)])
        let pages = (choice.items.count + Choice.page - 1) / Choice.page
        let title = "\(choice.syllable) → 한자 · 숫자·↩ 고르기 · ←→ 이동 · Esc 취소"
            + (pages > 1 ? " · \(first / Choice.page + 1)/\(pages)쪽" : "")
        feedback?.showChoices(page, selected: choice.index - first, title: title, near: { client.caretRect() })
    }

    /// A key while choosing hanja: true/false = consumed or not; nil = the choice ended and the key is handled normally.
    private func choose(keyCode: UInt16, modifiers: Modifiers, _ client: TextClient) -> Bool? {
        guard var current = choice else { return nil }
        let digits: [UInt16: Int] = [18: 1, 19: 2, 20: 3, 21: 4, 23: 5, 22: 6, 26: 7, 28: 8, 25: 9]
        let first = current.index / Choice.page * Choice.page
        func pick(_ index: Int) -> Bool {
            choice = nil
            feedback?.hide()
            client.insert(current.items[index], replacing: current.range)
            engine.forgetReconversion()
            diagnostics.correction("hanja")
            return true
        }
        if !modifiers.intersection([.command, .control, .option]).isEmpty {
            choice = nil
            feedback?.hide()
            return nil
        }
        switch keyCode {
        case 36, 76, 49: return pick(current.index)
        case 53, 51:
            choice = nil
            feedback?.hide()
            return true
        case 124: current.index = min(current.index + 1, current.items.count - 1)
        case 123: current.index = max(current.index - 1, 0)
        case 125, 121: current.index = min(first + Choice.page, current.items.count - 1)
        case 126, 116: current.index = max(first - Choice.page, 0)
        default:
            if let n = digits[keyCode], first + n - 1 < current.items.count { return pick(first + n - 1) }
            choice = nil
            feedback?.hide()
            return nil
        }
        choice = current
        showChoice(client)
        return true
    }

    // MARK: Results

    /// Applies an engine result: insert the commit, show or clear the composition, then the candidate window. `path`
    /// names what committed a word, for the diagnostics.
    private func apply(_ result: KeyResult, to client: TextClient?, path: String = "other", announce: Bool = true) {
        spacedCommit = nil
        var output = result
        if !result.failed, !result.commit.isEmpty, let client {
            output.commit = spacingText(result.commit, client)
            if path == "focus" { kept = output.commit }
        }
        if result.failed {
            // C-CMP-11: the engine returned the word as typed; insert it, keep a copy and fall back to pass-through.
            log("engine failure; switching to pass-through")
            diagnostics.failure()
            if !result.commit.isEmpty { kept = result.commit }
            _ = engine.setPaused(paused: true)
        }
        if !result.commit.isEmpty {
            diagnostics.commit(result.failed ? "failure" : path)
            if let shown, !result.failed { diagnostics.decision(shown.confidence, korean: shown.korean) }
            shown = nil
        }
        if !result.marked.isEmpty {
            shown = (result.confidence, engine.mode() == .korean || result.korean)
        }
        if let client {
            if !applyInline(output, client) {
            if !output.commit.isEmpty {
                client.insert(output.commit, replacing: nil)
                hasMarked = false
            }
            if !result.marked.isEmpty {
                client.setMarked(result.marked, uncertain: result.uncertain)
                hasMarked = true
            } else if hasMarked {
                client.setMarked("", uncertain: false)
                hasMarked = false
            }
            }
            if path == "space", result.marked.isEmpty, result.commit.hasSuffix(" "), output.commit != result.commit {
                rememberSpacing(original: result.commit, corrected: output.commit, client)
            }
            show(result, client)
            if result.failed {
                feedback?.showNotice("입력기 오류로 일시정지했습니다. 입력 메뉴에서 다시 켜세요", near: { client.caretRect() },
                                     sticky: false)
            }
        }
        if result.learned { save() }
        if !result.commit.isEmpty { checkField() }
        syncMode(client, announce: announce)
    }

    /// C-CTX-07: shows the composition as text in a search field and replaces it as it changes; the commit replaces it
    /// for good. Returns false when the word goes the usual way: not a search field, the setting off, a word that started
    /// as a composition, or a client that cannot say where the caret is. If the app changed the text in the meantime
    /// (autocomplete, a click), that text stays and the rest of the word is shown as a composition: no key is lost.
    private func applyInline(_ result: KeyResult, _ client: TextClient) -> Bool {
        if inline == nil {
            guard field == .search, Settings.searchInline, !hasMarked, !result.marked.isEmpty,
                  let caret = client.selectedRange() else { return false }
            if !result.commit.isEmpty { client.insert(result.commit, replacing: nil) }
            let start = client.selectedRange()?.location ?? caret.location + (result.commit as NSString).length
            client.insert(result.marked, replacing: nil)
            inline = (NSRange(location: start, length: (result.marked as NSString).length), result.marked)
            return true
        }
        guard let current = inline else { return false }
        // A client that cannot read its text back is trusted; one that reads something else changed it.
        if let now = client.text(in: current.range), now != current.text {
            log("search field: the app changed the text being composed; keeping it")
            inline = nil
            return false
        }
        if !result.commit.isEmpty {
            client.insert(result.commit, replacing: current.range)
            inline = nil
            guard !result.marked.isEmpty else { return true }
            var next = result
            next.commit = ""
            if !applyInline(next, client) {
                client.setMarked(result.marked, uncertain: result.uncertain)
                hasMarked = true
            }
            return true
        }
        client.insert(result.marked, replacing: current.range)
        inline = result.marked.isEmpty ? nil
            : (NSRange(location: current.range.location, length: (result.marked as NSString).length), result.marked)
        return true
    }

    /// Candidate window (D12: only when the decision is uncertain, unless the user wants it always) or the reservation
    /// a switch key made with nothing composed (22 §5.1a).
    private func show(_ result: KeyResult, _ client: TextClient) {
        guard let feedback else { return }
        let preview = result.failed ? result.marked : spacingText(result.marked, client)
        if !result.marked.isEmpty, preview != result.marked {
            let undo = Settings.binding(.candidate).map { "Space 확정 후 \($0.label) 원문 복원" } ?? "Space 확정"
            feedback.showCandidate(preview, hint: "띄어쓰기 · " + undo,
                                   near: { client.caretRect() })
        } else if !result.marked.isEmpty, !result.alternative.isEmpty, result.alternative != result.marked,
           result.uncertain || Settings.candidateAlways {
            let keys = Settings.semicolon && engine.mode() == .auto ? "; / F2 → " : "F2 → "
            let evidence = result.label.isEmpty ? "" : result.label + " · "
            feedback.showCandidate(result.marked, hint: evidence + keys + result.alternative + "   Space 확정",
                                   near: { client.caretRect() })
        } else if result.marked.isEmpty, let korean = engine.reserved() {
            feedback.showNotice(korean ? "다음 단어 한글" : "다음 단어 영문", near: { client.caretRect() }, sticky: true)
        } else {
            feedback.hide()
        }
    }
}
