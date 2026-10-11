// Unit tests for KeyMap and ShiftSpaceDetector (no AppKit). Built and run by build.sh --test.
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
check(act(120, &d) == .candidate, "F2 candidate")
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

if failures.isEmpty {
    print("PASS: \(count) KeyMap assertions")
} else {
    print("FAIL: \(failures.count) of \(count)")
    for f in failures { print("  - \(f)") }
    exit(1)
}
