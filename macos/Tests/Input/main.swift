// InputCore with a text buffer standing in for InputMethodKit's client: composition and commits, F2 reconversion,
// Shift+F2 selection flip, app exclusion, the switch gestures (⇧Space, right ⌘, Caps Lock with and without the monitor),
// mode input sources and the candidate window. No AppKit; built and run by build.sh --test.
import CoreGraphics
import Foundation

var count = 0
var failures: [String] = []
func check(_ condition: Bool, _ label: String) {
    count += 1
    if !condition { failures.append(label) }
}

let suite = "hanautomata-input-tests-\(ProcessInfo.processInfo.processIdentifier)"
Settings.defaults = UserDefaults(suiteName: suite)!
let dir = FileManager.default.temporaryDirectory.appendingPathComponent(suite)
try? FileManager.default.removeItem(at: dir)

/// A text field: `text` holds committed text, `marked` the composition shown at the caret.
final class Buffer: TextClient {
    var bundleIdentifier = "com.example.editor"
    let text = NSMutableString()
    var caret = 0
    var selection = 0
    var marked = ""
    var uncertain = false
    var readable = true
    var enabledModes: Set<String> = [ModeID.base]
    var selectedModes: [String] = []

    var all: String { text as String }

    func insert(_ string: String, replacing range: NSRange?) {
        let target = range ?? NSRange(location: caret, length: selection)
        text.replaceCharacters(in: target, with: string)
        caret = target.location + (string as NSString).length
        selection = 0
        marked = ""
    }

    func setMarked(_ string: String, uncertain: Bool) {
        marked = string
        self.uncertain = uncertain
    }

    func selectedRange() -> NSRange? { readable ? NSRange(location: caret, length: selection) : nil }

    func text(in range: NSRange) -> String? {
        readable && NSMaxRange(range) <= text.length ? text.substring(with: range) : nil
    }

    func caretRect() -> CGRect? { CGRect(x: 100, y: 500, width: 1, height: 16) }

    func selectMode(_ id: String) -> Bool {
        guard enabledModes.contains(id) else { return false }
        selectedModes.append(id)
        return true
    }
}

final class Window: Feedback {
    var candidate: (text: String, hint: String)?
    var notice: String?
    var mode: String?
    var choices: (items: [String], selected: Int)?

    func showMode(_ text: String, near rect: @escaping Caret) { mode = text }

    func showChoices(_ items: [String], selected: Int, title: String, near rect: @escaping Caret) {
        choices = (items, selected)
    }

    func showCandidate(_ text: String, hint: String, near rect: @escaping Caret) {
        candidate = (text, hint)
        notice = nil
    }

    func showNotice(_ text: String, near rect: @escaping Caret, sticky: Bool) {
        notice = text
        candidate = nil
    }

    func hide() {
        candidate = nil
        notice = nil
        choices = nil
    }
}

let codes = Dictionary(uniqueKeysWithValues: KeyMap.printable.map { ($0.value.0, $0.key) })
enum Code {
    static let space: UInt16 = 49, f2: UInt16 = 120, enter: UInt16 = 36, left: UInt16 = 123, backspace: UInt16 = 51
    static let escape: UInt16 = 53
    static let leftShift: UInt16 = 56
}

final class Harness {
    let engine: Engine
    let core: InputCore
    let window = Window()
    var caps: [Bool] = []
    var saves = 0
    var time: TimeInterval = 100

    init(_ name: String) {
        Settings.appModes = [:]  // each harness starts without remembered app modes
        engine = Engine(home: "/Users/홍길동", storePath: dir.appendingPathComponent("\(name).json").path)
        core = InputCore(engine: engine)
        core.feedback = window
        core.save = { [unowned self] in saves += 1 }
        core.setCapsLock = { [unowned self] in caps.append($0) }
    }

    func client(_ app: String = "com.example.editor") -> Buffer {
        let buffer = Buffer()
        buffer.bundleIdentifier = app
        core.activate(buffer, capsLocked: false)
        return buffer
    }

    @discardableResult
    func key(_ code: UInt16, _ modifiers: Modifiers = [], _ buffer: Buffer, repeating: Bool = false) -> Bool {
        core.keyDown(keyCode: code, modifiers: modifiers, isRepeat: repeating, client: buffer)
    }

    func type(_ text: String, _ buffer: Buffer) {
        for c in text { key(codes[c]!, [], buffer) }
    }

    func flags(_ code: UInt16, _ modifiers: Modifiers, _ buffer: Buffer, after: TimeInterval = 0.05) {
        time += after
        _ = core.flagsChanged(keyCode: code, modifiers: modifiers, time: time, client: buffer)
    }

    @discardableResult
    func shiftSpace(_ buffer: Buffer) -> Bool {
        flags(Code.leftShift, [.shift], buffer)
        let handled = key(Code.space, [.shift], buffer)
        flags(Code.leftShift, [], buffer)
        return handled
    }

    func rightCommandTap(_ buffer: Buffer, hold: TimeInterval = 0.1) {
        flags(KeyCode.rightCommand, [.command, .rightCommand], buffer)
        flags(KeyCode.rightCommand, [], buffer, after: hold)
    }
}

// Composition and Space commit.
let h = Harness("main")
var b = h.client()
h.type("dkssud", b)
check(b.marked == "안녕" && b.all == "", "the composition is marked, not in the text")
check(h.key(Code.space, [], b) && b.all == "안녕 " && b.marked == "", "Space commits with a space")

// F2 right after a Space commit swaps the word before the caret and remembers it (C-COR-02).
check(h.key(Code.f2, [], b) && b.all == "dkssud " && b.caret == 7, "F2 swaps the word before the caret")
check(h.saves == 1 && h.engine.learnedCount() == 1, "the swap is learned and saved")
check(h.key(Code.f2, [], b) && b.all == "안녕 ", "F2 again swaps back")
check(h.window.notice == nil && h.window.candidate == nil, "a swap shows nothing")

// The word is no longer right before the caret: nothing changes, a notice explains.
h.type("gksrmf", b)
h.key(Code.space, [], b)
check(b.all == "안녕 한글 ", "second word")
b.caret = 0
check(h.key(Code.f2, [], b) && b.all == "안녕 한글 " && h.window.notice == "직전 단어를 찾지 못해 바꾸지 않았습니다",
      "F2 after the caret moved changes nothing and says so")
check(h.engine.reconversion() == nil, "and forgets the word")
b.caret = b.text.length
h.type("dkssud", b)
h.key(Code.space, [], b)
b.readable = false
check(h.key(Code.f2, [], b) && b.all.hasSuffix("안녕 ") && h.window.notice != nil, "an unreadable client changes nothing")
b.readable = true
h.key(Code.left, [], b)
check(!h.key(Code.f2, [], b), "F2 with nothing to swap passes to the app")

// Shift+F2 flips the selection in place (C-COR-03).
b = h.client()
b.insert("dkssud gksrmf", replacing: nil)
b.caret = 0
b.selection = 6
check(h.key(Code.f2, [.shift], b) && b.all == "안녕 gksrmf", "Shift+F2 flips the selected text")
b.caret = 3
b.selection = 6
check(h.key(Code.f2, [.shift], b) && b.all == "안녕 한글", "flips the selection only")
check(h.key(Code.f2, [.shift], b) && h.window.notice == "바꿀 글자를 먼저 선택하세요" && b.all == "안녕 한글",
      "nothing selected: notice, no change")
b.readable = false
check(h.key(Code.f2, [.shift], b) && h.window.notice == "선택한 글자를 읽지 못했습니다", "unreadable selection: notice")
b.readable = true
h.type("rm", b)
check(b.marked == "rm", "a new client starts without the previous word's context")
check(h.key(Code.f2, [.shift], b) && b.marked == "그", "while composing, Shift+F2 swaps the candidate like F2")
h.core.commitPending(b)

// App exclusion (C-CTX-11).
let other = h.client()
other.bundleIdentifier = "com.example.excluded"
h.type("dkssud", other)
h.core.toggleExclusion(other)
check(other.all == "안녕" && Settings.excludedApps == ["com.example.excluded"], "excluding the app commits first")
check(!h.key(codes["r"]!, [], other) && other.marked == "", "an excluded app gets every key untouched")
check(!h.shiftSpace(other) && h.engine.reserved() == nil, "including the switch keys")
h.core.toggleExclusion(other)
check(Settings.excludedApps.isEmpty && h.key(codes["r"]!, [], other), "removing it restores composition")
h.core.commitPending(other)

// Right ⌘ tap (M-03): alone and short switches; a shortcut or a long hold does not.
b = h.client()
h.type("dkssud", b)
h.rightCommandTap(b)
check(b.marked == "dkssud", "right ⌘ tap flips the word")
h.rightCommandTap(b)
check(b.marked == "안녕", "a second tap restores it")
h.flags(KeyCode.rightCommand, [.command, .rightCommand], b)
h.key(codes["c"]!, [.command], b)
h.flags(KeyCode.rightCommand, [], b)
check(b.all == "안녕" && b.marked == "" && h.engine.reserved() == nil, "⌘C commits and does not switch")
h.rightCommandTap(b, hold: 0.5)
check(h.engine.reserved() == nil, "a long hold is not a tap")
h.flags(Code.leftShift, [.shift], b)
h.flags(KeyCode.rightCommand, [.shift, .command, .rightCommand], b)
h.flags(KeyCode.rightCommand, [.shift], b)
h.flags(Code.leftShift, [], b)
check(h.engine.reserved() == nil, "⇧ with right ⌘ is not a tap")
Settings.rightCommand = false
h.rightCommandTap(b)
check(h.engine.reserved() == nil, "the tap can be turned off")
Settings.rightCommand = true
h.rightCommandTap(b)
check(h.engine.reserved() == true && h.window.notice == "다음 단어 한글",
      "a tap with nothing composed reserves the next word (Korean after a shortcut commit)")
h.rightCommandTap(b)
check(h.engine.reserved() == nil && h.window.notice == nil, "a second tap cancels the reservation")

// ⇧Space (C-MOD-11) with nothing composed: reservation shown near the caret.
b = h.client()
h.engine.cancel()
check(h.shiftSpace(b) && h.engine.reserved() == true && h.window.notice == "다음 단어 한글", "⇧Space reserves Korean first")
h.type("dkssud", b)
check(b.marked == "안녕" && h.window.notice == nil, "the reserved word composes and the notice goes")
h.core.commitPending(b)

// Caps Lock without Input Monitoring: the lock turned on alone switches and is turned off; Shift+Caps Lock keeps it.
b = h.client()
h.type("dkssud", b)
h.caps = []
h.flags(KeyCode.capsLock, [.capsLock], b)
check(b.marked == "dkssud" && h.caps == [false], "Caps Lock switches and turns the lock off")
h.flags(KeyCode.capsLock, [], b)
check(b.marked == "dkssud" && h.caps == [false], "the lock going off again does nothing")
h.flags(KeyCode.capsLock, [.capsLock, .shift], b)
check(b.marked == "dkssud" && h.caps == [false] && h.core.caps.locked, "Shift+Caps Lock locks capitals")
h.flags(KeyCode.capsLock, [], b)
check(b.marked == "dkssud" && !h.core.caps.locked, "Caps Lock while locked only unlocks")
Settings.capsLock = false
h.flags(KeyCode.capsLock, [.capsLock], b)
check(b.marked == "dkssud" && h.caps == [false], "turned off: Caps Lock is an ordinary lock")
h.flags(KeyCode.capsLock, [], b)
Settings.capsLock = true

// Caps Lock with the monitor: exact press and release (C-MOD-10).
h.core.capsMonitored = true
h.flags(KeyCode.capsLock, [.capsLock], b)
check(b.marked == "dkssud" && h.caps == [false], "monitored: InputMethodKit lock changes are ignored")
h.core.capsPressed(at: 1.0, shift: false, client: b)
h.core.capsReleased(at: 1.12, client: b)
check(b.marked == "안녕" && h.caps == [false, false], "short press switches and unlocks")
h.core.capsPressed(at: 2.0, shift: false, client: b)
h.core.capsReleased(at: 2.6, client: b)
check(b.marked == "안녕" && h.caps.last == true && h.core.caps.locked, "long press locks capitals without switching")
h.core.capsPressed(at: 3.0, shift: false, client: b)
h.core.capsReleased(at: 3.1, client: b)
check(b.marked == "안녕" && h.caps.last == false && !h.core.caps.locked, "a press while locked only unlocks")
h.core.capsPressed(at: 4.0, shift: true, client: b)
h.core.capsReleased(at: 4.1, client: b)
check(b.marked == "안녕" && h.caps.last == true, "Shift+Caps Lock locks")
h.core.capsPressed(at: 5.0, shift: false, client: b)
h.core.capsReleased(at: 5.1, client: b)
let before = h.caps.count
h.core.capsPressed(at: 6.0, shift: false, client: nil)
h.core.capsReleased(at: 6.1, client: nil)
check(h.caps.count == before && b.marked == "안녕", "inactive: Caps Lock is left to the system")
h.core.capsMonitored = false
h.core.commitPending(b)

// Pause (C-MOD-04): every key and switch key passes.
b = h.client()
h.core.setPaused(true, client: b)
check(!h.key(codes["r"]!, [], b) && b.marked == "", "paused: text passes")
check(!h.shiftSpace(b) && h.engine.reserved() == nil, "paused: ⇧Space is a plain space")
h.caps = []
h.flags(KeyCode.capsLock, [.capsLock], b)
check(h.caps.isEmpty, "paused: Caps Lock is a plain lock")
h.flags(KeyCode.capsLock, [], b)
h.core.setPaused(false, client: b)

// Mode input sources (C-MOD-08): fixed modes toggle on a switch key; enabled sources are selected for their icon.
b = h.client()
b.enabledModes = [ModeID.base, ModeID.id(.korean)]
h.core.setMode(.korean, client: b)
check(b.selectedModes == [ModeID.id(.korean)] && Settings.mode == .korean, "menu mode selects its source and is saved")
h.type("dkssud", b)
h.shiftSpace(b)
check(b.all == "안녕" && h.engine.mode() == .english && Settings.mode == .english, "⇧Space in Korean mode commits and toggles")
check(b.selectedModes == [ModeID.id(.korean)], "a source that is not added is not selected")
h.core.setMode(.auto, client: b)
check(b.selectedModes.last == ModeID.base && h.engine.mode() == .auto, "back to automatic")

// The system reports the selected mode source; repeats are ignored, at launch only a fixed mode overrides the saved one.
let s = Harness("system")
Settings.mode = .korean
_ = s.engine.setMode(mode: .korean)
let sb = s.client()
s.core.systemSelected(modeID: ModeID.base, client: sb)
check(s.engine.mode() == .korean, "launch report of the automatic source keeps the saved mode")
s.core.systemSelected(modeID: ModeID.id(.english), client: sb)
check(s.engine.mode() == .english && Settings.mode == .english, "choosing the English source applies it")
s.core.systemSelected(modeID: ModeID.id(.english), client: sb)
s.core.setMode(.korean, client: sb)
s.core.systemSelected(modeID: ModeID.id(.english), client: sb)
check(s.engine.mode() == .korean, "a repeated report after our own change is ignored")
s.core.systemSelected(modeID: ModeID.base, client: sb)
check(s.engine.mode() == .auto, "a different source applies")
s.core.systemSelected(modeID: "com.apple.keylayout.ABC", client: sb)
check(s.engine.mode() == .auto, "other input sources are ignored")

// Candidate window (C-CMP-02 · D12): uncertain words show the alternative, evidence and keys.
let c = Harness("candidate")
let cb = c.client()
c.type("rm", cb)
check(cb.marked == "rm" && cb.uncertain, "near-tie stays raw with a dotted underline")
let hint = c.window.candidate?.hint ?? ""
check(c.window.candidate?.text == "rm" && hint.contains("확인 필요") && hint.contains("; / F2 → 그"), "uncertain: candidate window")
c.key(Code.space, [], cb)
check(c.window.candidate == nil, "commit hides it")
c.engine.cancel()
c.type("dkssud", cb)
check(c.window.candidate == nil, "a certain word shows no window")
Settings.candidateAlways = true
c.key(Code.backspace, [], cb)
check(c.window.candidate != nil, "always: every word with an alternative")
Settings.candidateAlways = false
c.key(Code.enter, [], cb)

// Focus changes commit; deactivation hides the window.
c.type("rm", cb)
c.core.deactivate(cb)
check(cb.all.hasSuffix("rm") && cb.marked == "" && c.window.candidate == nil, "deactivation commits and hides")

// Switch meanings (C-MOD-02): cycle walks the modes, toggle leaves automatic for the other language.
let m = Harness("meaning")
var mb = m.client()
mb.enabledModes = Set(ModeID.all.map { ModeID.id($0) })
m.engine.setSwitchKind(kind: .cycle)
m.type("gksrmf", mb)
check(m.shiftSpace(mb) && mb.all == "한글" && m.engine.mode() == .korean, "cycle: commits, then Korean")
check(mb.selectedModes == [ModeID.id(.korean)] && Settings.mode == .korean, "the new mode's source is selected and saved")
m.shiftSpace(mb)
m.shiftSpace(mb)
check(m.engine.mode() == .auto && mb.selectedModes.last == ModeID.base, "cycle: English, then automatic")
m.engine.setSwitchKind(kind: .toggle)
m.type("dkssud", mb)
m.shiftSpace(mb)
check(m.engine.mode() == .english && mb.all == "한글안녕", "toggle: a Korean word on screen goes to English")
m.engine.setSwitchKind(kind: .singleShot)
m.core.setMode(.auto, client: mb)

// F12 pass-through (C-MOD-04): toggles even while paused, never in an excluded app.
mb = m.client()
check(m.key(KeyCode.f12, [], mb) && m.engine.paused() && m.window.notice?.hasPrefix("일시정지") == true, "F12 pauses")
check(!m.key(codes["r"]!, [], mb) && mb.marked == "", "paused: keys pass")
check(m.key(KeyCode.f12, [], mb) && !m.engine.paused() && m.window.notice == "다시 판별합니다", "F12 again resumes")
check(m.key(KeyCode.f12, [], mb, repeating: true) && !m.engine.paused(), "auto-repeat does not toggle again")
check(!m.key(KeyCode.f12, [.command], mb) && !m.engine.paused(), "⌘F12 is the app's")
Settings.pauseKey = false
check(!m.key(KeyCode.f12, [], mb) && !m.engine.paused(), "the key can be turned off")
Settings.pauseKey = true

// Unicode text another program injects (C-CTX-08) goes in as it is, after the word being composed.
m.type("dkssud", mb)
check(!m.core.keyDown(keyCode: 0, modifiers: [], isRepeat: false, injected: true, client: mb), "injected text passes")
check(mb.all == "안녕" && mb.marked == "" && m.core.diagnostics.snapshot.passed["injected"] == 1,
      "the word in progress is committed first, and the pass is counted")

// Engine fault (C-CMP-11, injected): the word typed so far goes in literally, the key passes, the copy is kept and the
// input method falls back to pass-through.
let f = Harness("fault")
var fb = f.client()
f.type("dkssud", fb)
f.engine.failNextCallForTesting()
check(!f.key(codes["q"]!, [], fb), "the faulting key passes to the app")
check(fb.all == "dkssud" && fb.marked == "" && f.engine.paused(), "the word goes in as typed and the engine pauses")
check(f.core.kept == "dkssud" && f.window.notice?.contains("오류") == true && f.core.diagnostics.snapshot.failures == 1,
      "a copy is kept and the user is told")
fb = f.client()
f.core.reinsertKept(fb)
check(fb.all == "dkssud" && f.core.kept.isEmpty, "the kept text can be inserted again, once")
f.core.setPaused(false, client: fb)
f.type("rk", fb)
f.engine.failNextCallForTesting()
f.core.deactivate(fb)
check(fb.all == "dkssudrk" && f.engine.paused(), "a fault while focus leaves keeps the text too")
f.core.setPaused(false, client: fb)
fb = f.client()
f.type("gksrmf", fb)
f.core.deactivate(fb)
check(f.core.kept == "한글", "text committed when focus leaves is kept for the menu (the client may be gone)")

// Secure fields (C-CTX-01) and the learning pause (C-LEX-09): choices typed there are not recorded.
let p = Harness("secure")
var secure = true
p.core.secureInput = { secure }
let pb = p.client()
p.type("rm", pb)
p.key(Code.f2, [], pb)
p.key(Code.space, [], pb)
check(p.engine.learnedCount() == 0 && p.engine.learningPaused() && p.core.diagnostics.snapshot.secureKeys > 0,
      "secure field: an explicit choice is not learned")
secure = false
p.key(Code.enter, [], pb)  // no preceding word: "rm" is a near-tie again
p.type("rm", pb)
p.key(Code.f2, [], pb)
p.key(Code.space, [], pb)
let learned = p.engine.learnedCount()
check(learned > 0 && !p.engine.learningPaused(), "outside it learning resumes")
Settings.learningPaused = true
p.key(Code.enter, [], pb)
p.type("dh", pb)
p.key(Code.f2, [], pb)
p.key(Code.space, [], pb)
check(p.engine.learnedCount() == learned && p.engine.learningPaused(), "learning pause: nothing new is recorded")
p.engine.cancel()
p.type("rm", pb)
check(pb.marked == "그", "learning pause: remembered choices still apply (got \(pb.marked))")
Settings.learningPaused = false
p.key(Code.enter, [], pb)

// Per-app modes (C-MOD-05) and profiles (C-MOD-06 · C-CTX-02/03).
let a = Harness("apps")
var appA = a.client("com.example.a")
a.core.setMode(.english, client: appA)
var appB = a.client("com.example.b")
check(a.engine.mode() == .english, "a new app without a remembered mode keeps the current one")
a.core.setMode(.auto, client: appB)
appA = a.client("com.example.a")
check(a.engine.mode() == .english && Settings.appModes["com.example.a"] == "english", "app A gets its English mode back")
appB = a.client("com.example.b")
check(a.engine.mode() == .auto, "app B gets its automatic mode back")
let vm = a.client("com.vmware.fusion")
check(a.engine.mode() == .english, "a virtual machine starts in English (built-in profile)")
check(Settings.mode == .auto, "a profile's start mode is not saved as the user's mode")
_ = vm
let term = a.client("com.apple.Terminal")
check(a.engine.mode() == .auto, "a terminal's profile has no start mode: the user's mode returns, not the VM's English")
a.core.setMode(.english, client: term)
_ = a.client("com.vmware.fusion")
let plain = a.client("com.example.other")
check(a.engine.mode() == .english && Settings.mode == .english, "a mode the user chose is global for apps without one")
a.core.setMode(.auto, client: plain)
_ = a.client("com.apple.Terminal")
check(a.engine.mode() == .english && Settings.mode == .auto, "restoring an app's remembered mode leaves the user's mode")
check(a.engine.sensitivity() == "conservative", "a terminal judges conservatively (built-in profile)")
let editor = a.client("com.example.editor")
check(a.engine.sensitivity() == Settings.sensitivity, "other apps use the global sensitivity")
Settings.profiles = ["com.example.strict": AppProfile(mode: "auto", sensitivity: nil, strict: true)]
let strict = a.client("com.example.strict")
a.type("qhanf", strict)
check(strict.marked == "qhanf", "strict profile: a statistical estimate stays as typed")
a.key(Code.f2, [], strict)
check(strict.marked == "보물", "and F2 offers the Hangul")
a.key(Code.enter, [], strict)
_ = a.client("com.example.editor")
a.type("qhanf", editor)
check(editor.marked == "보물", "other apps convert it (strict is per app)")
a.key(Code.enter, [], editor)
Settings.profiles = [:]
Settings.builtInProfiles = false
_ = a.client("com.example.c")
let vm2 = a.client("org.example.vm")
check(Settings.profile(for: "com.vmware.fusion") == nil && Settings.profile(for: "org.example.vm") == nil, "built-ins can be off")
_ = vm2
Settings.builtInProfiles = true
Settings.profiles = ["org.example.vm": AppProfile(mode: "korean", sensitivity: "aggressive", strict: false)]
Settings.appModes = [:]
_ = a.client("com.example.c")
_ = a.client("org.example.vm")
check(a.engine.mode() == .korean && a.engine.sensitivity() == "aggressive", "a user profile sets the start mode and sensitivity")
Settings.profiles = [:]
Settings.perAppMode = false
Settings.appModes = [:]

// Mode shortcut (C-MOD-12), mode notice (C-MOD-08), remapped correction keys (C-COR-07).
let k = Harness("keys")
let kb = k.client()
Settings.bindings = [.modeKorean: KeyBinding(38, KeyBinding.control), .candidate: KeyBinding(99)]
check(k.key(38, [.control], kb) && k.engine.mode() == .korean && k.window.mode == "가 한글", "⌃J: Korean mode, shown by the caret")
k.window.mode = nil
k.core.setMode(.auto, client: kb)
check(k.window.mode == "가A 자동", "menu changes are shown too")
Settings.modeNotice = false
k.window.mode = nil
k.core.setMode(.english, client: kb)
check(k.window.mode == nil, "the notice can be turned off")
Settings.modeNotice = true
k.core.setMode(.auto, client: kb)
k.type("rm", kb)
check(!k.key(Code.f2, [], kb) && kb.all == "rm", "F2 moved away: it commits and goes to the app")
k.type("rm", kb)
check(k.key(99, [], kb) && kb.marked == "그", "F3 now swaps the candidate")
k.key(Code.enter, [], kb)
Settings.bindings = [.switchKey: KeyBinding(49, KeyBinding.control | KeyBinding.option)]
check(k.key(49, [.control, .option], kb) && k.engine.reserved() == true, "a custom switch key reserves the next word")
k.key(49, [.control, .option], kb)
Settings.bindings = [:]

// Extra modifier taps (C-MOD-03 · M-18): left Shift alone switches when turned on.
Settings.extraTaps = [TapKey.leftShift.rawValue]
k.flags(56, [.shift, .leftShift], kb)
k.flags(56, [], kb)
check(k.engine.reserved() == true, "left Shift tap switches")
k.flags(56, [.shift, .leftShift], kb)
k.key(codes["a"]!, [.shift], kb)
k.flags(56, [], kb)
check(kb.marked == "A" || kb.marked == "ㅁ", "Shift held for a capital is not a tap")
k.key(Code.enter, [], kb)
Settings.extraTaps = []
k.flags(56, [.shift, .leftShift], kb)
k.flags(56, [], kb)
check(k.engine.reserved() == nil, "off by default")

// Esc vi mode (C-MOD-07 · M-09): commits, goes English, Esc reaches the app; the switch key comes back.
Settings.escVi = "all"
let v = Harness("vi")
let vb = v.client("com.apple.Terminal")
v.core.setMode(.auto, client: vb)
v.type("dkssud", vb)
check(!v.key(53, [], vb) && vb.all == "안녕" && v.engine.mode() == .english, "Esc: commit, English, Esc passes")
check(!v.key(codes["d"]!, [], vb), "vim commands pass in English")
check(v.shiftSpace(vb) && v.engine.mode() == .auto && v.core.viReturn == nil, "⇧Space returns to the previous mode")
Settings.perAppMode = true
v.type("gksrmf", vb)
check(!v.key(33, [.control], vb) && v.engine.mode() == .english && vb.all.hasSuffix("한글"), "⌃[ works like Esc")
check(Settings.mode == .auto, "vi mode's English is not saved as the user's mode")
_ = v.client("com.example.notes")
check(v.engine.mode() == .auto, "the next app gets the user's mode, not vi mode's English")
_ = v.client("com.apple.Terminal")
check(v.engine.mode() == .english, "the terminal gets its vi English back")
Settings.perAppMode = false
Settings.appModes = [:]
v.core.setMode(.auto, client: vb)
Settings.escVi = "apps"
let vOther = v.client("com.example.editor")
v.type("dkssud", vOther)
check(v.key(53, [], vOther) && v.engine.mode() == .auto && vOther.marked == "" && vOther.all == "",
      "apps: other apps keep the normal Esc (it cancels the composition)")
Settings.escVi = "off"

// Hanja (C-LEX-06 · M-08): ⌥↩ after a syllable shows numbered hanja; a digit or ↩ picks, Esc cancels.
let j = Harness("hanja")
let jb = j.client()
j.type("gks", jb)
check(j.key(36, [.option], jb) && jb.all == "한" && j.window.choices?.items.first == "寒", "⌥↩ commits and lists hanja")
check(j.key(19, [], jb) && jb.all == "恨" && j.window.choices == nil, "2 picks the second")
check(!j.key(36, [.option], jb), "no Hangul before the caret: the key is the app's")
j.type("gks", jb)
j.key(36, [.option], jb)
j.key(124, [], jb)
check(j.window.choices?.selected == 1, "→ moves the highlight")
check(j.key(36, [], jb) && jb.all.hasSuffix("恨"), "↩ picks the highlighted one")
j.type("gks", jb)
j.key(36, [.option], jb)
check(j.key(53, [], jb) && jb.all.hasSuffix("한") && j.window.choices == nil, "Esc cancels")
j.key(36, [.option], jb)
j.type("r", jb)
check(j.window.choices == nil && jb.marked == "r", "another key cancels and types")
j.key(Code.enter, [], jb)
jb.text.setString("가나다 한")
jb.caret = 4
jb.selection = 1
check(j.key(36, [.option], jb) && j.window.choices != nil, "a selected syllable converts too")
j.key(18, [], jb)
check(jb.all.hasPrefix("가나다 ") && !jb.all.hasSuffix("한"), "the selection is replaced")

// The last three words flip at once (C-COR-04).
let r3 = Harness("recent")
let rb = r3.client()
r3.core.setMode(.english, client: rb)
for c in "hi dkssud gksrmf qksrkqtmqslek" { press(r3, c, rb) }
r3.core.setMode(.auto, client: rb)
check(r3.key(Code.f2, [.option], rb) && rb.all == "hi 안녕 한글 반갑습니다", "⌥F2 flips the last three words (\(rb.all))")
check(r3.key(Code.f2, [.option], rb) && rb.all == "hi dkssud gksrmf qksrkqtmqslek", "again flips them back")

// ⌥ English (M-06) and ₩ (M-07).
let o = Harness("option")
let ob = o.client()
Settings.optionKey = .english
o.type("dkssud", ob)
check(o.key(codes["a"]!, [.option], ob) && ob.all == "안녕a", "⌥A types a after committing")
Settings.optionKey = .special
check(!o.key(codes["a"]!, [.option], ob), "special: ⌥A goes to the app")
Settings.wonSign = true
o.core.setMode(.korean, client: ob)
o.type("rk", ob)
check(o.key(50, [], ob) && ob.all.hasSuffix("가₩"), "` types ₩ in the Korean mode")
Settings.wonSign = false
o.core.setMode(.auto, client: ob)

// Sebeolsik layout through the shell (C-LAY-02).
o.core.setKeyboard(.sebeolsik390, client: ob)
let ob2 = o.client()
for c in "kfs" { press(o, c, ob2) }
check(ob2.marked == "간", "390 keys compose")
o.core.setKeyboard(.dubeolsik, client: ob2)
check(ob2.all == "간", "changing the layout commits")

// Caret-text language (C-DET-08, optional): Korean text before the caret tips a near-tie for the first word.
let ct = Harness("caret")
let cb2 = ct.client()
cb2.text.setString("어제 보낸 메일 확인했어요 ")
cb2.caret = cb2.text.length
ct.type("dh", cb2)
check(cb2.marked == "dh", "off by default: the near-tie stays raw")
ct.key(Code.escape, [], cb2)
Settings.caretContext = true
ct.type("dh", cb2)
check(cb2.marked == "오", "on: Korean text before the caret tips it")
ct.key(Code.escape, [], cb2)
Settings.caretContext = false

// Settings travel in the export bundle (C-LEX-04).
Settings.sensitivity = "aggressive"
Settings.profiles = ["com.example.x": AppProfile(mode: "english", sensitivity: nil, strict: true)]
let exported = Settings.exported()
Settings.sensitivity = "balanced"
Settings.profiles = [:]
check(Settings.imported(exported) >= 2 && Settings.sensitivity == "aggressive" && Settings.profiles["com.example.x"]?.strict == true,
      "settings round-trip through the bundle JSON")
check(Settings.imported("not json") == 0, "a broken settings object changes nothing")
Settings.sensitivity = "balanced"
Settings.profiles = [:]

// Search fields (C-CTX-07, experimental): the composition goes in as text the app's search sees, replaced as it changes.
check(FieldKind.classify(role: "AXTextField", subrole: "AXSearchField", identifier: "") == .search, "AXSearchField is a search field")
check(FieldKind.classify(role: "AXTextField", subrole: "", identifier: "WEB_BROWSER_ADDRESS_AND_SEARCH_FIELD") == .search,
      "Safari's address-and-search field is one")
check(FieldKind.classify(role: "AXTextField", subrole: "", identifier: "") == .standard, "a plain text field is not")
do {
    Settings.searchInline = true
    let plain = Harness("search-plain"), search = Harness("search-inline")
    var asked = 0
    search.core.checkField = { asked += 1 }
    let p = plain.client("com.apple.Safari"), q = search.client("com.apple.Safari")
    check(asked == 1, "activation asks what the field is")
    search.core.fieldChanged(.search)
    // Whatever the composition would show, the search field holds as text: after every key, through commits, Backspace,
    // F2 swaps, punctuation and digits.
    var keys: [UInt16] = []
    for word in ["dkssudgktpdy", "hello", "rkfk,", "123", "gksrmf", "test."] {
        keys += word.map { codes[$0]! } + [Code.space]
    }
    keys.insert(contentsOf: [Code.f2], at: 13)
    keys += [codes["r"]!, codes["k"]!, Code.backspace, Code.backspace, codes["d"]!, codes["k"]!, Code.backspace]
    var same = true
    for key in keys {
        plain.key(key, [], p)
        search.key(key, [], q)
        if q.all != p.all + p.marked || !q.marked.isEmpty { same = false }
    }
    check(same, "a search field shows as text exactly what the composition shows, key by key")
    check(q.all == p.all + p.marked && q.all.hasPrefix("dkssudgktpdy hello "), "same text at the end, F2 swap included (\(q.all))")
    check(asked > 3, "each commit asks again (field changes are seen between words)")
    plain.core.deactivate(p)
    search.core.deactivate(q)
    check(q.all == p.all && q.marked.isEmpty, "and the same after focus leaves")

    // The app changes the text under the composition (autocomplete, a click): it stays, the rest is a composition.
    var r = search.client("com.apple.Safari")
    search.core.fieldChanged(.search)
    search.type("dk", r)
    r.text.replaceCharacters(in: NSRange(location: 0, length: 1), with: "X")
    search.type("s", r)
    check(r.all == "X" && r.marked == "안", "a changed field keeps its text and shows the rest as a composition")
    search.key(Code.space, [], r)
    check(r.all == "X안 " && r.marked == "", "nothing typed is lost")

    // A word that started as a composition stays one; the setting off or an unreadable client keeps compositions.
    r = search.client("com.apple.Safari")
    search.type("dk", r)
    search.core.fieldChanged(.search)
    search.type("s", r)
    check(r.all == "" && r.marked == "안", "a word already shown as a composition keeps that form")
    search.key(Code.space, [], r)
    search.core.fieldChanged(.search)
    r.readable = false
    search.type("dk", r)
    check(r.marked == "아", "a client that cannot say where the caret is gets a composition")
    search.key(Code.space, [], r)
    r.readable = true
    Settings.searchInline = false
    search.type("dk", r)
    check(r.marked == "아", "the setting off: compositions as before")
    search.key(Code.space, [], r)
    Settings.searchInline = true

    // Focus leaving commits the text in place, once.
    r = search.client("com.apple.Safari")
    search.core.fieldChanged(.search)
    search.type("dkssud", r)
    search.core.deactivate(r)
    check(r.all == "안녕" && r.marked == "", "focus leaving commits in place")
    check(search.client("com.apple.Safari") != nil && search.core.field == .standard, "a new client starts as a plain field")
    Settings.searchInline = false
}

// C-CTX-06 replay: 500 sentences of held-out words typed with switch keys, F2 swaps, clicks, focus moves and key repeats.
// The app's text, read back as keys (Hangul runs decomposed), must be exactly what was typed: nothing lost, duplicated or
// reordered.
func corpus(_ name: String, column: Int) -> [String] {
    guard CommandLine.arguments.count > 1,
          let text = try? String(contentsOfFile: CommandLine.arguments[1] + "/" + name, encoding: .utf8) else { return [] }
    return text.split(separator: "\n").compactMap { line in
        guard !line.hasPrefix("#") else { return nil }
        let field = line.split(separator: "\t").map(String.init)
        guard field.count > column else { return nil }
        let word = field[column]
        return word.allSatisfy({ $0.isASCII && $0.isLetter }) ? word : nil
    }
}
let koreanKeys = corpus("heldout-korean-1000.tsv", column: 0)
let englishWords = corpus("heldout-english-1000.tsv", column: 0).filter { $0 == $0.lowercased() }
check(koreanKeys.count > 900 && englishWords.count > 800, "replay corpora found (pass tests/corpora as the argument)")

struct Random {
    var state: UInt64
    mutating func next(_ n: Int) -> Int {
        state = state &* 6364136223846793005 &+ 1442695040888963407
        return Int((state >> 33) % UInt64(n))
    }
}

/// The app side of a key the input method passed: it types the character itself.
func press(_ r: Harness, _ c: Character, _ buffer: Buffer, repeating: Bool = false) {
    let lower = Character(c.lowercased())
    let code = c == " " ? Code.space : c == "\n" ? Code.enter : codes[lower]!
    let shift = c.isUppercase
    if !r.key(code, shift ? [.shift] : [], buffer, repeating: repeating) {
        buffer.insert(String(c), replacing: nil)
    }
}

func asKeys(_ text: String, _ engine: Engine) -> String {
    var out = "", run = ""
    for c in text {
        if let u = c.unicodeScalars.first?.value, (0xAC00...0xD7A3).contains(u) || (0x3131...0x318E).contains(u) {
            run.append(c)
        } else {
            out += run.isEmpty ? "" : engine.flip(text: run)
            run = ""
            out.append(c)
        }
    }
    return out + (run.isEmpty ? "" : engine.flip(text: run))
}

if !koreanKeys.isEmpty && !englishWords.isEmpty {
    let r = Harness("replay")
    var rng = Random(state: 20261012)
    var buffers = [r.client()]
    var typed = ""
    var events: [String: Int] = [:]
    for _ in 0..<500 {
        for w in 0..<(4 + rng.next(6)) {
            let word = rng.next(10) < 6 ? koreanKeys[rng.next(koreanKeys.count)] : englishWords[rng.next(englishWords.count)]
            if rng.next(10) == 0 {
                r.shiftSpace(buffers.last!)
                events["reserve", default: 0] += 1
            }
            let middle = word.count > 1 ? 1 + rng.next(word.count - 1) : word.count
            for (i, c) in word.enumerated() {
                if i == middle {
                    switch rng.next(20) {
                    case 0, 1:
                        r.shiftSpace(buffers.last!)
                        events["flip", default: 0] += 1
                    case 2:
                        r.rightCommandTap(buffers.last!)
                        events["rightCommand", default: 0] += 1
                    case 3:
                        r.core.commitPending(buffers.last!)
                        events["click", default: 0] += 1
                    case 4:
                        press(r, c, buffers.last!, repeating: true)
                        typed.append(c)
                        events["repeat", default: 0] += 1
                    default: break
                    }
                }
                press(r, c, buffers.last!)
                typed.append(c)
            }
            if rng.next(12) == 0 {
                r.core.deactivate(buffers.last!)
                buffers.append(r.client())
                events["focus", default: 0] += 1
            } else {
                press(r, " ", buffers.last!)
                typed.append(" ")
                if rng.next(15) == 0 {
                    r.key(Code.f2, [], buffers.last!)
                    events["f2", default: 0] += 1
                }
            }
            _ = w
        }
        press(r, "\n", buffers.last!)
        typed.append("\n")
    }
    r.core.deactivate(buffers.last!)
    let text = buffers.map(\.all).joined()
    let back = asKeys(text, r.engine)
    let same = back == typed
    if !same, let i = zip(back, typed).enumerated().first(where: { $0.element.0 != $0.element.1 })?.offset {
        print("replay differs at \(i): typed …\(typed.dropFirst(max(0, i - 20)).prefix(40))… got …\(back.dropFirst(max(0, i - 20)).prefix(40))…")
    }
    check(same && buffers.allSatisfy { $0.marked.isEmpty }, "replay: \(typed.count) keys, nothing lost, duplicated or reordered")
    check(events.values.allSatisfy { $0 > 10 } && events.count == 7, "replay exercised every event kind: \(events)")
    let converted = text.unicodeScalars.filter { (0xAC00...0xD7A3).contains($0.value) }.count
    check(converted > 1000, "replay converted Korean words (\(converted) syllables)")

    // Content-free diagnostics (C-OPS-02): the saved counters contain none of the typed words.
    let saved = dir.appendingPathComponent("diagnostics.json")
    try? r.core.diagnostics.save(to: saved)
    let json = (try? String(contentsOf: saved, encoding: .utf8)) ?? ""
    let sample = Set(typed.split(whereSeparator: { $0 == " " || $0 == "\n" }).filter { $0.count >= 4 }.prefix(300).map(String.init))
    check(!json.isEmpty && !sample.contains(where: { json.contains($0) }) && !json.contains("안녕"),
          "diagnostics hold no typed text")
    let snapshot = r.core.diagnostics.snapshot
    check((snapshot.commits["space"] ?? 0) > 2000 && (snapshot.commits["focus"] ?? 0) > 0 && !snapshot.decisions.isEmpty,
          "diagnostics count commits by path and decisions by strength")
    check(Diagnostics.load(from: saved) == snapshot, "diagnostics round-trip through the file")
    print("  replay: \(typed.count) keys · \(buffers.count) fields · events \(events.sorted { $0.key < $1.key })")
    print("  " + Diagnostics.report(snapshot).split(separator: "\n").prefix(6).joined(separator: "\n  "))
}

// C-OPS-05: the input method's own share of a key leaves out the time the app takes to answer.
do {
    let d = Diagnostics()
    d.latency(nanoseconds: 3_000_000, waiting: 2_900_000)
    d.latency(nanoseconds: 4_000_000, waiting: 3_950_000)
    let s = d.snapshot
    check(s.latency.percentile(0.99) == 4_000 && s.own?.percentile(0.99) == 100 && s.own?.maxMicros == 100,
          "latency splits into the total and the input method's own share, never reported above the maximum")
    check(Diagnostics.report(s).contains("without waiting for the app: p50 ≤ 0.05ms · p99 ≤ 0.10ms"),
          "--diagnose shows the own share")
}

// C-OPS-05: shell key handling (InputCore + engine, the text buffer as client) over 10,000 keys.
do {
    let t = Harness("latency")
    let tb = t.client()
    let words = koreanKeys.isEmpty ? ["dkssud", "hello", "gksrmf"] : Array(koreanKeys.prefix(300) + englishWords.prefix(300))
    var samples: [UInt64] = []
    var i = 0
    while samples.count < 10_000 {
        for c in words[i % words.count] + " " {
            let start = DispatchTime.now().uptimeNanoseconds
            press(t, c, tb)
            samples.append(DispatchTime.now().uptimeNanoseconds - start)
        }
        i += 1
    }
    samples.sort()
    let p50 = Double(samples[samples.count / 2]) / 1e6, p99 = Double(samples[samples.count * 99 / 100]) / 1e6
    print(String(format: "  latency over %d keys: p50 %.3fms · p99 %.3fms · max %.3fms", samples.count, p50, p99,
                 Double(samples.last!) / 1e6))
    check(p99 <= 5, "key handling p99 within the 5 ms budget")
}

try? FileManager.default.removeItem(at: dir)
UserDefaults.standard.removePersistentDomain(forName: suite)

if failures.isEmpty {
    print("PASS: \(count) input-logic assertions")
} else {
    print("FAIL: \(failures.count) of \(count)")
    for f in failures { print("  - \(f)") }
    exit(1)
}
