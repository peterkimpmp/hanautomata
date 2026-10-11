// Unit tests for KeyMap and the switch gestures (no AppKit). Built and run by build.sh --test.
import Foundation

var count = 0
var failures: [String] = []

func check(_ condition: Bool, _ label: String) {
    count += 1
    if !condition { failures.append(label) }
}

func act(_ code: UInt16, shift: Bool = false, caps: Bool = false, command: Bool = false, option: Bool = false,
         repeating: Bool = false, enabled: Bool = true, _ d: inout ShiftSpaceDetector) -> KeyAction {
    KeyMap.action(keyCode: code, shift: shift, capsLock: caps, command: command, control: false, option: option,
                  isRepeat: repeating, shiftSpaceEnabled: enabled, detector: &d)
}

var d = ShiftSpaceDetector()
// Letters: literal applies Shift xor Caps Lock, the Dubeolsik key applies Shift only (Native.TryAscii).
check(act(15, &d) == .text(raw: "r", key: "r"), "plain r")
check(act(15, shift: true, &d) == .text(raw: "R", key: "R"), "shift r → ㄲ key")
check(act(15, caps: true, &d) == .text(raw: "R", key: "r"), "caps r keeps the ㄱ key")
check(act(15, shift: true, caps: true, &d) == .text(raw: "r", key: "R"), "shift+caps r")
check(act(18, &d) == .text(raw: "1", key: "1") && act(18, shift: true, &d) == .text(raw: "!", key: "!"), "digit row")
check(act(41, &d) == .text(raw: ";", key: ";") && act(39, shift: true, &d) == .text(raw: "\"", key: "\""), "symbols")
check(act(50, caps: true, &d) == .text(raw: "`", key: "`"), "caps does not shift symbols")
check(KeyMap.printable.count == 26 + 10 + 11, "47 printable keys like TryAscii")
// Control keys.
check(act(36, &d) == .enter && act(76, &d) == .enter, "return and keypad enter")
check(act(48, &d) == .tab && act(51, &d) == .backspace && act(53, &d) == .escape, "tab backspace escape")
check(act(120, &d) == .other, "F2 is a command key, resolved through the bindings")
for code: UInt16 in [123, 124, 125, 126, 115, 119, 116, 121, 117] { check(act(code, &d) == .navigation, "navigation \(code)") }
check(act(8, command: true, &d) == .shortcut && act(0, option: true, &d) == .shortcut, "⌘C and ⌥a are shortcuts")
check(act(122, &d) == .other && act(83, &d) == .other, "F1 and keypad digits commit and pass")
// ⇧Space: Shift alone then Space switches; a key typed while Shift is held makes Space ordinary (C-MOD-11).
d = ShiftSpaceDetector()
check(act(49, &d) == .space, "plain space")
d.flagsChanged(shift: true)
check(act(49, shift: true, &d) == .switchKey, "shift then space switches")
check(act(49, shift: true, repeating: true, &d) == .ignore, "auto-repeat of the gesture is swallowed")
check(act(49, shift: true, &d) == .switchKey, "a second press while still holding Shift switches again")
d.flagsChanged(shift: false)
check(act(49, &d) == .space, "after release, space is a space")
d.flagsChanged(shift: true)
check(act(34, shift: true, &d) == .text(raw: "I", key: "I"), "typing I with Shift")
check(act(49, shift: true, &d) == .space, "`I am`: Space after a shifted letter stays a space")
d.flagsChanged(shift: false)
d.flagsChanged(shift: true)
check(act(49, shift: true, enabled: false, &d) == .space, "disabled gesture types a space")
d.flagsChanged(shift: false)
d.flagsChanged(shift: true)
check(act(49, shift: true, command: true, &d) == .shortcut, "⇧⌘Space stays a shortcut")
d.reset()
check(act(49, shift: true, &d) == .space, "reset disarms (Shift held across activation)")

// Shift+F2 flips the selection; F2 alone is the candidate key (C-COR-02·03).
func cmd(_ code: UInt16, shift: Bool = false, option: Bool = false, command: Bool = false,
         _ bindings: [Command: KeyBinding?] = [:]) -> Command? {
    KeyMap.command(keyCode: code, shift: shift, control: false, option: option, command: command, bindings: bindings)
}
check(cmd(120) == .candidate && cmd(120, shift: true) == .flip, "F2 and Shift+F2 by default")
check(cmd(111) == .pause && cmd(120, option: true) == .recent && cmd(36, option: true) == .hanja, "F12 · ⌥F2 · ⌥↩")
check(cmd(120, command: true) == nil && cmd(36) == nil, "other modifier sets are the app's")

// Right ⌘ tap: alone, released within 0.3 s, nothing in between (M-03).
var tap = ModifierTap()
check(!tap.changed(isTapKey: true, down: true, others: false, at: 0) && tap.changed(isTapKey: true, down: false, others: false, at: 0.2),
      "right ⌘ tap")
_ = tap.changed(isTapKey: true, down: true, others: false, at: 1)
check(!tap.changed(isTapKey: true, down: false, others: false, at: 1.31), "held past the window")
_ = tap.changed(isTapKey: true, down: true, others: false, at: 2)
tap.keyDown()
check(!tap.changed(isTapKey: true, down: false, others: false, at: 2.1), "⌘ with a key is a shortcut")
_ = tap.changed(isTapKey: true, down: true, others: false, at: 3)
check(!tap.changed(isTapKey: false, down: false, others: true, at: 3.05) && !tap.changed(isTapKey: true, down: false, others: true, at: 3.1),
      "another modifier in between")
_ = tap.changed(isTapKey: true, down: true, others: true, at: 4)
check(!tap.changed(isTapKey: true, down: false, others: false, at: 4.1), "pressed while another modifier is held")
check(!tap.changed(isTapKey: true, down: false, others: false, at: 5), "release without a press")

// Caps Lock (C-MOD-10): monitored press/release, and the fallback that only sees lock changes.
var capsGesture = CapsLockGesture()
capsGesture.press(at: 0, shift: false)
check(capsGesture.release(at: 0.15) == .switchLanguage && !capsGesture.locked, "short press switches")
capsGesture.press(at: 1, shift: false)
check(capsGesture.release(at: 1.5) == .lock && capsGesture.locked, "0.5 s press locks")
capsGesture.press(at: 2, shift: false)
check(capsGesture.release(at: 2.05) == .unlock && !capsGesture.locked, "press while locked unlocks, no switch")
capsGesture.press(at: 3, shift: true)
check(capsGesture.release(at: 3.05) == .lock, "Shift+Caps Lock locks")
capsGesture.press(at: 4, shift: true)
check(capsGesture.release(at: 4.05) == .unlock, "Shift+Caps Lock while locked unlocks")
check(capsGesture.release(at: 5) == .none, "release without a press")
var fallback = CapsLockGesture()
check(fallback.lockChanged(on: true, shift: false) == .switchLanguage && !fallback.locked, "fallback: lock on alone switches")
check(fallback.lockChanged(on: false, shift: false) == .none, "fallback: our own unlock is quiet")
check(fallback.lockChanged(on: true, shift: true) == .none && fallback.locked, "fallback: Shift+Caps Lock locks")
check(fallback.lockChanged(on: false, shift: false) == .none && !fallback.locked, "fallback: unlocking")
fallback.sync(locked: true)
fallback.press(at: 6, shift: false)
check(fallback.release(at: 6.1) == .unlock, "sync with the actual lock state")

// Unicode text injected by another program (C-CTX-08): characters the layout does not give for the key.
check(KeyMap.isInjected(keyCode: 0, characters: "h", expected: "a"), "key code 0 carrying 'h' is injected")
check(!KeyMap.isInjected(keyCode: 0, characters: "a", expected: "a"), "the keyboard's own character is not")
check(!KeyMap.isInjected(keyCode: 0, characters: "A", expected: "A"), "Caps Lock or Shift capitals are not")
check(KeyMap.isInjected(keyCode: 0, characters: "가", expected: nil), "non-ASCII text without a layout is injected")
check(!KeyMap.isInjected(keyCode: 0, characters: "q", expected: nil), "ASCII without a layout is trusted")
check(!KeyMap.isInjected(keyCode: 123, characters: "\u{F702}", expected: "\u{1C}"), "arrows are never injected text")
check(!KeyMap.isInjected(keyCode: 36, characters: "\r", expected: "\r"), "Enter is not checked")
check(KeyMap.isInjected(keyCode: 49, characters: "안녕", expected: " "), "a Space event carrying text is injected")
check(!KeyMap.isInjected(keyCode: 0, characters: nil, expected: "a"), "no characters: not injected")
check(!KeyMap.isInjected(keyCode: 15, characters: "p", expected: "r"), "another key's ASCII mismatch is trusted (layout quirk)")

// Remapping (C-COR-07): a moved command frees its old key; an unset one has no key; mode shortcuts (C-MOD-12).
let moved: [Command: KeyBinding?] = [.candidate: KeyBinding(99), .pause: nil, .modeKorean: KeyBinding(38, KeyBinding.control)]
check(cmd(99, moved) == .candidate && cmd(120, moved) == nil, "F3 now swaps candidates; F2 goes to the app")
check(cmd(111, moved) == nil, "the pause key can be unset")
check(KeyMap.command(keyCode: 38, shift: false, control: true, option: false, command: false, bindings: moved) == .modeKorean,
      "⌃J selects the Korean mode")
check(KeyBinding(120, KeyBinding.shift).label == "⇧F2" && KeyBinding(36, KeyBinding.option).label == "⌥↩"
      && KeyBinding(0, KeyBinding.control | KeyBinding.command).label == "⌃⌘A", "binding labels")
check(KeyBinding(stored: KeyBinding(120, 5).stored) == KeyBinding(120, 5) && KeyBinding(stored: "x") == nil, "stored form")

// ⌥ + key (M-06): special characters by default, the plain key, or the English letter.
check(KeyMap.action(keyCode: 0, shift: false, capsLock: false, command: false, control: false, option: true,
                    isRepeat: false, shiftSpaceEnabled: true, optionKey: .special, detector: &d) == .shortcut, "⌥A: special")
check(KeyMap.action(keyCode: 0, shift: false, capsLock: false, command: false, control: false, option: true,
                    isRepeat: false, shiftSpaceEnabled: true, optionKey: .ignore, detector: &d) == .text(raw: "a", key: "a"),
      "⌥A ignored: a")
check(KeyMap.action(keyCode: 0, shift: true, capsLock: false, command: false, control: false, option: true,
                    isRepeat: false, shiftSpaceEnabled: true, optionKey: .english, detector: &d) == .english("A"),
      "⌥⇧A English: A")
check(KeyMap.action(keyCode: 0, shift: false, capsLock: false, command: true, control: false, option: true,
                    isRepeat: false, shiftSpaceEnabled: true, optionKey: .english, detector: &d) == .shortcut,
      "⌘⌥A stays a shortcut")

if failures.isEmpty {
    print("PASS: \(count) KeyMap assertions")
} else {
    print("FAIL: \(failures.count) of \(count)")
    for f in failures { print("  - \(f)") }
    exit(1)
}
