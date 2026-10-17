import Carbon
import Cocoa
import InputMethodKit

/// InputMethodKit's client as a `TextClient`.
final class IMKClient: TextClient {
    let base: AnyObject
    private let input: IMKTextInput

    var identity: ObjectIdentifier { ObjectIdentifier(base) }

    init?(_ sender: Any?) {
        guard let input = sender as? (IMKTextInput & NSObjectProtocol) else { return nil }
        self.input = input
        base = input
    }

    /// The client's app, asked once per client: every key reads it (exclusion, profiles, diagnostics) and the call goes to
    /// the app.
    var bundleIdentifier: String {
        let id = ObjectIdentifier(base)
        if let cached = IMKClient.lastBundle, cached.client == id { return cached.bundle }
        let bundle = IMKClient.timed { input.bundleIdentifier() } ?? ""
        IMKClient.lastBundle = (id, bundle)
        return bundle
    }

    private static var lastBundle: (client: ObjectIdentifier, bundle: String)?

    /// A new client may reuse a gone client's address: forget the cached app whenever a client activates.
    static func forgetBundle() { lastBundle = nil }

    private static let notFound = NSRange(location: NSNotFound, length: NSNotFound)

    /// Time spent waiting for the app to answer InputMethodKit calls since the last reset: each call is a round trip to
    /// the app, so this part of a key's latency is the app's (C-OPS-05).
    static var waited: UInt64 = 0

    private static func timed<T>(_ call: () -> T) -> T {
        let start = DispatchTime.now().uptimeNanoseconds
        defer { waited += DispatchTime.now().uptimeNanoseconds - start }
        return call()
    }

    func insert(_ text: String, replacing range: NSRange?) {
        IMKClient.timed { input.insertText(text, replacementRange: range ?? IMKClient.notFound) }
    }

    func setMarked(_ text: String, uncertain: Bool) {
        var style = NSUnderlineStyle.single
        if uncertain { style.insert(.patternDot) }
        let marked = NSAttributedString(string: text, attributes: text.isEmpty ? [:] : [.underlineStyle: style.rawValue])
        IMKClient.timed {
            input.setMarkedText(marked, selectionRange: NSRange(location: text.utf16.count, length: 0),
                                replacementRange: IMKClient.notFound)
        }
    }

    func selectedRange() -> NSRange? {
        let range = IMKClient.timed { input.selectedRange() }
        return range.location == NSNotFound ? nil : range
    }

    func text(in range: NSRange) -> String? {
        IMKClient.timed { input.attributedSubstring(from: range)?.string }
    }

    func caretRect() -> CGRect? {
        var rect = NSRect.zero
        _ = IMKClient.timed { input.attributes(forCharacterIndex: 0, lineHeightRectangle: &rect) }
        return rect == .zero ? nil : rect
    }

    func selectMode(_ id: String) -> Bool {
        guard InputSources.isEnabled(id) else { return false }
        IMKClient.timed { input.selectMode(id) }
        return true
    }
}

/// The keyboard layout InputMethodKit uses with this input method, to tell keys from injected text (C-CTX-08).
enum KeyboardLayout {
    private static var data: Data?

    /// Re-reads the current layout (the user may change it between activations).
    static func refresh() {
        data = nil
        guard let source = TISCopyCurrentKeyboardLayoutInputSource()?.takeRetainedValue(),
              let pointer = TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData)
        else { return }
        data = Unmanaged<CFData>.fromOpaque(pointer).takeUnretainedValue() as Data
    }

    /// What the layout types for the key with Shift and Caps Lock only, nil when unknown.
    static func characters(keyCode: UInt16, shift: Bool, capsLock: Bool) -> String? {
        guard let data else { return nil }
        var state: UInt32 = 0
        if shift { state |= UInt32(shiftKey >> 8) }
        if capsLock { state |= UInt32(alphaLock >> 8) }
        var dead: UInt32 = 0
        var length = 0
        var chars = [UniChar](repeating: 0, count: 8)
        let status = data.withUnsafeBytes { raw -> OSStatus in
            guard let base = raw.baseAddress else { return OSStatus(paramErr) }
            return UCKeyTranslate(base.assumingMemoryBound(to: UCKeyboardLayout.self), keyCode, UInt16(kUCKeyActionDown),
                                  state, UInt32(LMGetKbdType()), OptionBits(kUCKeyTranslateNoDeadKeysMask), &dead,
                                  chars.count, &length, &chars)
        }
        return status == noErr ? String(utf16CodeUnits: chars, count: length) : nil
    }
}

extension Modifiers {
    /// NSEvent flags, plus the device bits that tell the left and right modifiers apart (NX_DEVICE…KEYMASK).
    init(_ flags: NSEvent.ModifierFlags) {
        var result: Modifiers = []
        if flags.contains(.shift) { result.insert(.shift) }
        if flags.contains(.capsLock) { result.insert(.capsLock) }
        if flags.contains(.control) { result.insert(.control) }
        if flags.contains(.option) { result.insert(.option) }
        if flags.contains(.command) { result.insert(.command) }
        if flags.contains(.function) { result.insert(.function) }
        let device = flags.rawValue
        if device & 0x08 != 0 { result.insert(.leftCommand) }
        if device & 0x10 != 0 { result.insert(.rightCommand) }
        if device & 0x02 != 0 { result.insert(.leftShift) }
        if device & 0x04 != 0 { result.insert(.rightShift) }
        if device & 0x40 != 0 { result.insert(.rightOption) }
        if device & 0x2000 != 0 { result.insert(.rightControl) }
        self = result
    }
}

/// One controller per client text session; all share `Shell.core`.
@objc(HanautomataInputController)
final class HanautomataInputController: IMKInputController {
    // Mouse downs too: with only key events InputMethodKit commits on clicks by itself, with more it leaves that to us.
    override func recognizedEvents(_ sender: Any!) -> Int {
        Int(NSEvent.EventTypeMask([.keyDown, .flagsChanged, .leftMouseDown, .rightMouseDown, .otherMouseDown]).rawValue)
    }

    override func activateServer(_ sender: Any!) {
        IMKClient.forgetBundle()
        guard let client = IMKClient(sender) else { return }
        Shell.activate(client)
    }

    override func deactivateServer(_ sender: Any!) {
        Shell.deactivate(IMKClient(sender))
    }

    /// The system asks for the composition to end (click elsewhere, focus change, menu).
    override func commitComposition(_ sender: Any!) {
        Shell.core.commitPending(IMKClient(sender))
    }

    /// The selected mode input source (menu bar, ⌃Space, or the current one when a client activates).
    override func setValue(_ value: Any!, forTag tag: Int, client sender: Any!) {
        guard tag == Int(kTSMDocumentInputModePropertyTag), let id = value as? String else {
            return super.setValue(value, forTag: tag, client: sender)
        }
        Shell.core.systemSelected(modeID: id, client: IMKClient(sender))
    }

    override func handle(_ event: NSEvent!, client sender: Any!) -> Bool {
        guard let event, let client = IMKClient(sender) else { return false }
        switch event.type {
        case .flagsChanged:
            return Shell.core.flagsChanged(keyCode: event.keyCode, modifiers: Modifiers(event.modifierFlags),
                                           time: event.timestamp, client: client)
        case .keyDown:
            let started = DispatchTime.now().uptimeNanoseconds
            IMKClient.waited = 0
            let modifiers = Modifiers(event.modifierFlags)
            let expected = KeyboardLayout.characters(keyCode: event.keyCode, shift: modifiers.contains(.shift),
                                                     capsLock: modifiers.contains(.capsLock))
            let injected = modifiers.intersection([.command, .control, .option]).isEmpty
                && KeyMap.isInjected(keyCode: event.keyCode, characters: event.characters, expected: expected)
            if let source = event.cgEvent?.getIntegerValueField(.eventSourceStateID) {
                Shell.core.diagnostics.source(state: source)
            }
            let handled = Shell.core.keyDown(keyCode: event.keyCode, modifiers: modifiers, isRepeat: event.isARepeat,
                                             injected: injected, client: client)
            let elapsed = DispatchTime.now().uptimeNanoseconds - started
            Shell.latency.record(nanoseconds: elapsed)
            Shell.core.diagnostics.latency(nanoseconds: elapsed, waiting: min(IMKClient.waited, elapsed))
            return handled
        case .leftMouseDown, .rightMouseDown, .otherMouseDown:
            Shell.core.commitPending(client)
            return false
        default:
            return false
        }
    }

    // MARK: - Input menu (M-17)

    override func menu() -> NSMenu! {
        let menu = NSMenu()
        let engine = Shell.engine
        func item(_ title: String, _ action: Selector, on: Bool = false, enabled: Bool = true) -> NSMenuItem {
            let item = NSMenuItem(title: title, action: action, keyEquivalent: "")
            item.state = on ? .on : .off
            item.isEnabled = enabled
            return item
        }
        let mode = engine.mode()
        menu.addItem(item("자동 판별", #selector(modeAuto(_:)), on: mode == .auto))
        menu.addItem(item("한글 고정", #selector(modeKorean(_:)), on: mode == .korean))
        menu.addItem(item("영문 고정", #selector(modeEnglish(_:)), on: mode == .english))
        menu.addItem(item("일시정지(그대로 입력)", #selector(togglePause(_:)), on: engine.paused()))
        if let client = IMKClient(client()), !client.bundleIdentifier.isEmpty {
            let name = InputSources.appName(client.bundleIdentifier)
            menu.addItem(item("\(name)에서 끄기", #selector(toggleExclusion(_:)), on: Shell.core.isExcluded(client)))
        }
        menu.addItem(.separator())
        menu.addItem(item("자동 띄어쓰기", #selector(toggleSpacing(_:)), on: Settings.autoSpacing))
        menu.addItem(item("내 표현…", #selector(showPersonalExpressions(_:))))
        menu.addItem(item("선택한 글자 한↔영 바꾸기  \(Settings.binding(.flip)?.label ?? "")", #selector(flipSelection(_:))))
        menu.addItem(item("최근 세 단어 한↔영 바꾸기  \(Settings.binding(.recent)?.label ?? "")", #selector(flipRecent(_:))))
        menu.addItem(item("클립보드 글자 한↔영 바꾸기", #selector(flipClipboard(_:))))
        menu.addItem(item("선택 영역 한국어 표기 검사 (실험)", #selector(proofreadSelection(_:))))
        menu.addItem(.separator())
        menu.addItem(item("선택을 기억", #selector(toggleLearning(_:)), on: engine.learningEnabled()))
        menu.addItem(item("학습 일시 중지(기억한 선택은 계속 적용)", #selector(toggleLearningPause(_:)), on: Settings.learningPaused,
                          enabled: engine.learningEnabled()))
        menu.addItem(item("최근 학습 되돌리기", #selector(undoLearning(_:)), enabled: engine.canUndoLearning()))
        menu.addItem(item("개인 학습 초기화…", #selector(resetLearning(_:))))
        let kept = Shell.core.kept
        if !kept.isEmpty {
            let preview = kept.count > 16 ? String(kept.prefix(16)) + "…" : kept
            menu.addItem(item("보관한 입력 다시 넣기: \(preview)", #selector(reinsertKept(_:))))
        }
        menu.addItem(.separator())
        menu.addItem(item("설정…", #selector(showPreferences(_:))))
        menu.addItem(item("다듬 \(Updates.current) 정보…", #selector(showAbout(_:))))
        menu.addItem(item("기억한 선택 \(engine.learnedCount())개", #selector(noop(_:)), enabled: false))
        if Settings.updateCheck, !Settings.latestVersion.isEmpty {
            menu.addItem(item("새 판 \(Settings.latestVersion) 내려받기…", #selector(openReleases(_:))))
        }
        let storeError = engine.storeError()
        if !storeError.isEmpty {
            menu.addItem(item("학습 파일 문제: \(storeError)", #selector(noop(_:)), enabled: false))
        }
        return menu
    }

    private func setMode(_ mode: InputMode) {
        Shell.core.setMode(mode, client: IMKClient(client()))
    }

    @objc func modeAuto(_ sender: Any?) { setMode(.auto) }
    @objc func showPersonalExpressions(_ sender: Any?) {
        Shell.core.commitPending(IMKClient(client()))
        PersonalAssistPanel.show()
    }
    @objc func toggleSpacing(_ sender: Any?) {
        Settings.autoSpacing.toggle()
        SettingsWindow.reload()
    }
    @objc func modeKorean(_ sender: Any?) { setMode(.korean) }
    @objc func modeEnglish(_ sender: Any?) { setMode(.english) }
    @objc func togglePause(_ sender: Any?) {
        Shell.core.setPaused(!Shell.engine.paused(), client: IMKClient(client()))
    }
    @objc func toggleExclusion(_ sender: Any?) {
        if let client = IMKClient(client()) { Shell.core.toggleExclusion(client) }
    }
    @objc func toggleLearning(_ sender: Any?) {
        Shell.engine.setLearningEnabled(enabled: !Shell.engine.learningEnabled())
        Settings.learning = Shell.engine.learningEnabled()
        Shell.saveSoon()
    }
    @objc func flipSelection(_ sender: Any?) {
        if let client = IMKClient(client()) { Shell.core.run(command: .flip, client: client) }
    }
    @objc func flipRecent(_ sender: Any?) {
        if let client = IMKClient(client()) { Shell.core.run(command: .recent, client: client) }
    }
    @objc func proofreadSelection(_ sender: Any?) {
        guard let original = IMKClient(client()), Shell.core.proofreadingAllowed, !Shell.core.isExcluded(original) else { return }
        let epoch = Shell.core.proofreadingEpoch
        let pid = NSWorkspace.shared.frontmostApplication?.processIdentifier
        let identity = String(describing: ObjectIdentifier(original.base))
        let read: () -> ProofreadingSelection? = { [weak self] in
            guard let self, let current = IMKClient(self.client()), current.base === original.base,
                  Shell.core.proofreadingEpoch == epoch, Shell.core.proofreadingAllowed, !Shell.core.isExcluded(current),
                  NSWorkspace.shared.frontmostApplication?.processIdentifier == pid,
                  let range = current.selectedRange(), range.length > 0, range.length <= 4096,
                  let text = current.text(in: range), text.utf16.count == range.length else { return nil }
            return ProofreadingSelection(text: text, document: identity, revision: String(epoch), range: range)
        }
        ProofreadingPanel.show(target: ProofreadingTarget(read: read, replace: { old, new in
            guard read() == old else { return false }
            original.insert(new, replacing: old.range); return true
        }))
    }
    /// C-COR-06: the clipboard's text flipped between Hangul and QWERTY keys, written back (the user asked for it).
    @objc func flipClipboard(_ sender: Any?) {
        let board = NSPasteboard.general
        guard let text = board.string(forType: .string), !text.isEmpty else { return }
        let flipped = Shell.engine.flip(text: text)
        board.clearContents()
        board.setString(flipped, forType: .string)
    }
    @objc func openReleases(_ sender: Any?) {
        NSWorkspace.shared.open(Updates.releases)
    }
    @objc func toggleLearningPause(_ sender: Any?) {
        Settings.learningPaused.toggle()
        Shell.core.updateRecording()
    }
    @objc func reinsertKept(_ sender: Any?) {
        Shell.core.reinsertKept(IMKClient(client()))
    }
    @objc func undoLearning(_ sender: Any?) {
        _ = Shell.engine.undoLearning()
        Shell.saveSoon()
    }
    @objc func resetLearning(_ sender: Any?) {
        NSApp.activate(ignoringOtherApps: true)
        let alert = NSAlert()
        alert.messageText = "개인 학습을 모두 지울까요?"
        alert.informativeText = "기억한 선택 \(Shell.engine.learnedCount())개가 지워집니다. 되돌릴 수 없습니다."
        alert.addButton(withTitle: "지우기")
        alert.addButton(withTitle: "취소")
        if alert.runModal() == .alertFirstButtonReturn {
            Shell.engine.clearLearning()
            Shell.saveSoon()
        }
    }
    override func showPreferences(_ sender: Any!) {
        SettingsWindow.show(firstRun: false)
    }
    @objc func showAbout(_ sender: Any?) {
        SettingsWindow.show(firstRun: false, tab: .about)
    }
    @objc func noop(_ sender: Any?) {}
}
