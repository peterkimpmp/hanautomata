import Cocoa
import InputMethodKit
import os

/// One controller per client text session (InputMethodKit). All of them share the process-wide `Shell.engine`; only one
/// client is active at a time and the composition is committed whenever focus leaves it.
@objc(HanautomataInputController)
final class HanautomataInputController: IMKInputController {
    private var shiftSpace = ShiftSpaceDetector()
    private var hasMarked = false

    override func recognizedEvents(_ sender: Any!) -> Int {
        Int(NSEvent.EventTypeMask([.keyDown, .flagsChanged]).rawValue)
    }

    override func activateServer(_ sender: Any!) {
        shiftSpace.reset()
        hasMarked = false
    }

    override func deactivateServer(_ sender: Any!) {
        apply(Shell.engine.commitPending(), to: sender as? IMKTextInput)
        shiftSpace.reset()
    }

    /// The system asks for the composition to end (click elsewhere, focus change, menu).
    override func commitComposition(_ sender: Any!) {
        apply(Shell.engine.commitPending(), to: sender as? IMKTextInput)
    }

    override func handle(_ event: NSEvent!, client sender: Any!) -> Bool {
        guard let event, let client = sender as? IMKTextInput else { return false }
        let flags = event.modifierFlags
        if event.type == .flagsChanged {
            shiftSpace.flagsChanged(shift: flags.contains(.shift))
            return false
        }
        guard event.type == .keyDown else { return false }
        let started = DispatchTime.now().uptimeNanoseconds
        let action = KeyMap.action(keyCode: event.keyCode, shift: flags.contains(.shift), capsLock: flags.contains(.capsLock),
                                   command: flags.contains(.command), control: flags.contains(.control),
                                   option: flags.contains(.option), isRepeat: event.isARepeat,
                                   shiftSpaceEnabled: Settings.shiftSpace, detector: &shiftSpace)
        let result: KeyResult
        switch action {
        case .ignore: return true
        case .text(let raw, let key): result = Shell.engine.handle(kind: .text, raw: String(raw), key: String(key))
        case .space: result = Shell.engine.handle(kind: .space, raw: "", key: "")
        case .switchKey: result = Shell.engine.handle(kind: .switch, raw: "", key: "")
        case .candidate: result = Shell.engine.handle(kind: .candidate, raw: "", key: "")
        case .backspace: result = Shell.engine.handle(kind: .backspace, raw: "", key: "")
        case .escape: result = Shell.engine.handle(kind: .escape, raw: "", key: "")
        case .enter: result = Shell.engine.handle(kind: .enter, raw: "", key: "")
        case .tab: result = Shell.engine.handle(kind: .tab, raw: "", key: "")
        case .navigation: result = Shell.engine.handle(kind: .navigation, raw: "", key: "")
        case .shortcut: result = Shell.engine.handle(kind: .shortcut, raw: "", key: "")
        case .other: result = Shell.engine.handle(kind: .other, raw: "", key: "")
        }
        apply(result, to: client)
        Shell.latency.record(nanoseconds: DispatchTime.now().uptimeNanoseconds - started)
        return result.handled
    }

    private func apply(_ result: KeyResult, to client: IMKTextInput?) {
        if result.failed {
            // C-CMP-11: never lose the user's text to an engine fault; fall back to pass-through.
            Shell.log.error("engine failure; switching to pass-through")
            _ = Shell.engine.setPaused(paused: true)
        }
        guard let client else { return }
        let notFound = NSRange(location: NSNotFound, length: NSNotFound)
        if !result.commit.isEmpty {
            client.insertText(result.commit, replacementRange: notFound)
            hasMarked = false
        }
        if !result.marked.isEmpty {
            var style = NSUnderlineStyle.single
            if result.uncertain { style.insert(.patternDot) }
            let text = NSAttributedString(string: result.marked, attributes: [.underlineStyle: style.rawValue])
            client.setMarkedText(text, selectionRange: NSRange(location: result.marked.utf16.count, length: 0),
                                 replacementRange: notFound)
            hasMarked = true
        } else if hasMarked {
            client.setMarkedText("", selectionRange: NSRange(location: 0, length: 0), replacementRange: notFound)
            hasMarked = false
        }
        if result.learned { Shell.saveSoon() }
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
        menu.addItem(.separator())
        let sensitivity = engine.sensitivity()
        menu.addItem(item("민감도: 보수", #selector(sensitivityConservative(_:)), on: sensitivity == "conservative"))
        menu.addItem(item("민감도: 표준", #selector(sensitivityBalanced(_:)), on: sensitivity == "balanced"))
        menu.addItem(item("민감도: 적극", #selector(sensitivityAggressive(_:)), on: sensitivity == "aggressive"))
        menu.addItem(.separator())
        menu.addItem(item("⇧Space로 전환", #selector(toggleShiftSpace(_:)), on: Settings.shiftSpace))
        menu.addItem(item("단어 뒤 ; 로 후보 전환", #selector(toggleSemicolon(_:)), on: Settings.semicolon))
        menu.addItem(item("선택을 기억", #selector(toggleLearning(_:)), on: engine.learningEnabled()))
        menu.addItem(item("최근 학습 되돌리기", #selector(undoLearning(_:)), enabled: engine.canUndoLearning()))
        menu.addItem(item("개인 학습 초기화…", #selector(resetLearning(_:))))
        menu.addItem(.separator())
        let version = Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "?"
        menu.addItem(item("Hanautomata \(version) · 기억한 선택 \(engine.learnedCount())개", #selector(noop(_:)), enabled: false))
        let storeError = engine.storeError()
        if !storeError.isEmpty {
            menu.addItem(item("학습 파일 문제: \(storeError)", #selector(noop(_:)), enabled: false))
        }
        return menu
    }

    private func setMode(_ mode: InputMode) {
        apply(Shell.engine.setMode(mode: mode), to: client())
        Settings.mode = mode
    }

    @objc func modeAuto(_ sender: Any?) { setMode(.auto) }
    @objc func modeKorean(_ sender: Any?) { setMode(.korean) }
    @objc func modeEnglish(_ sender: Any?) { setMode(.english) }
    @objc func togglePause(_ sender: Any?) {
        apply(Shell.engine.setPaused(paused: !Shell.engine.paused()), to: client())
    }
    @objc func sensitivityConservative(_ sender: Any?) { Settings.sensitivity = Shell.engine.setSensitivity(name: "conservative") }
    @objc func sensitivityBalanced(_ sender: Any?) { Settings.sensitivity = Shell.engine.setSensitivity(name: "balanced") }
    @objc func sensitivityAggressive(_ sender: Any?) { Settings.sensitivity = Shell.engine.setSensitivity(name: "aggressive") }
    @objc func toggleShiftSpace(_ sender: Any?) { Settings.shiftSpace.toggle() }
    @objc func toggleSemicolon(_ sender: Any?) {
        Settings.semicolon.toggle()
        Shell.engine.setSemicolonShortcut(enabled: Settings.semicolon)
    }
    @objc func toggleLearning(_ sender: Any?) {
        Shell.engine.setLearningEnabled(enabled: !Shell.engine.learningEnabled())
        Settings.learning = Shell.engine.learningEnabled()
        Shell.saveSoon()
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
    @objc func noop(_ sender: Any?) {}
}
