// Physical-key mapping, user key bindings and the switch gestures (⇧Space, modifier taps, Caps Lock), kept free of AppKit
// so Tests/KeyMap can run them.
// Characters follow the Windows app's Native.TryAscii on a US/Korean physical layout: the literal character applies
// Shift and Caps Lock, the Dubeolsik key applies Shift only (Caps Lock must not turn ㄱ into ㄲ).

import Foundation

enum KeyAction: Equatable {
    case text(raw: Character, key: Character)
    case space
    case switchKey
    case backspace
    case escape
    case enter
    case tab
    case navigation
    case shortcut
    /// ⌥ + key typed as the plain English letter (M-06 "English").
    case english(Character)
    case other
    /// Consumed without any effect (auto-repeat of a switch gesture).
    case ignore
}

struct ShiftSpaceDetector {
    private(set) var shiftDown = false
    private var clean = false

    /// Shift pressed alone arms the gesture; releasing it disarms.
    mutating func flagsChanged(shift: Bool) {
        if shift && !shiftDown { clean = true }
        if !shift { clean = false }
        shiftDown = shift
    }

    /// True when this Space is "Shift, then Space with no other key in between" (C-MOD-11). Any other key typed while
    /// Shift is held makes the following Space an ordinary space (`I am`, `Hello World`).
    mutating func keyDown(isSpace: Bool) -> Bool {
        if isSpace && shiftDown && clean { return true }
        if shiftDown { clean = false }
        return false
    }

    mutating func reset() {
        shiftDown = false
        clean = false
    }
}

enum KeyMap {
    static let space: UInt16 = 49

    /// ANSI key codes of printable keys → (plain, shifted), as TryAscii's virtual-key table.
    static let printable: [UInt16: (Character, Character)] = {
        var map: [UInt16: (Character, Character)] = [:]
        let letters: [(UInt16, Character)] = [
            (0, "a"), (11, "b"), (8, "c"), (2, "d"), (14, "e"), (3, "f"), (5, "g"), (4, "h"), (34, "i"), (38, "j"), (40, "k"),
            (37, "l"), (46, "m"), (45, "n"), (31, "o"), (35, "p"), (12, "q"), (15, "r"), (1, "s"), (17, "t"), (32, "u"), (9, "v"),
            (13, "w"), (7, "x"), (16, "y"), (6, "z"),
        ]
        for (code, c) in letters { map[code] = (c, Character(c.uppercased())) }
        let digits: [(UInt16, Character, Character)] = [
            (29, "0", ")"), (18, "1", "!"), (19, "2", "@"), (20, "3", "#"), (21, "4", "$"), (23, "5", "%"), (22, "6", "^"),
            (26, "7", "&"), (28, "8", "*"), (25, "9", "("),
        ]
        let symbols: [(UInt16, Character, Character)] = [
            (41, ";", ":"), (24, "=", "+"), (43, ",", "<"), (27, "-", "_"), (47, ".", ">"), (44, "/", "?"), (50, "`", "~"),
            (33, "[", "{"), (42, "\\", "|"), (30, "]", "}"), (39, "'", "\""),
        ]
        for (code, plain, shifted) in digits + symbols { map[code] = (plain, shifted) }
        return map
    }()

    static let navigation: Set<UInt16> = [123, 124, 125, 126, 115, 119, 116, 121, 117]

    /// What ⌥ + key does (M-06): the layout's special character (default), the key as if ⌥ were not held, or the plain
    /// English letter.
    enum OptionKey: String {
        case special, ignore, english
    }

    static func action(keyCode: UInt16, shift: Bool, capsLock: Bool, command: Bool, control: Bool, option: Bool,
                       isRepeat: Bool, shiftSpaceEnabled: Bool, optionKey: OptionKey = .special,
                       detector: inout ShiftSpaceDetector) -> KeyAction {
        let isSpace = keyCode == space
        let gesture = detector.keyDown(isSpace: isSpace)
        if command || control { return .shortcut }
        if option {
            switch optionKey {
            case .special: return .shortcut
            case .english:
                guard let (plain, shifted) = printable[keyCode] else { return .shortcut }
                return .english(plain.isLetter && shift != capsLock || !plain.isLetter && shift ? shifted : plain)
            case .ignore: break
            }
        }
        if isSpace {
            if gesture && shiftSpaceEnabled { return isRepeat ? .ignore : .switchKey }
            return .space
        }
        switch keyCode {
        case 36, 76: return .enter
        case 48: return .tab
        case 51: return .backspace
        case 53: return .escape
        default: break
        }
        if navigation.contains(keyCode) { return .navigation }
        guard let (plain, shifted) = printable[keyCode] else { return .other }
        if plain.isLetter {
            let raw = (shift != capsLock) ? shifted : plain
            return .text(raw: raw, key: shift ? shifted : plain)
        }
        let c = shift ? shifted : plain
        return .text(raw: c, key: c)
    }
}

/// A key with its modifiers, as the user sets it for a command (C-COR-07 · C-MOD-12 · C-MOD-03). Caps Lock is ignored.
struct KeyBinding: Equatable {
    static let shift: UInt8 = 1, control: UInt8 = 2, option: UInt8 = 4, command: UInt8 = 8

    var keyCode: UInt16
    var modifiers: UInt8

    init(_ keyCode: UInt16, _ modifiers: UInt8 = 0) {
        self.keyCode = keyCode
        self.modifiers = modifiers
    }

    static func modifiers(shift: Bool, control: Bool, option: Bool, command: Bool) -> UInt8 {
        (shift ? KeyBinding.shift : 0) | (control ? KeyBinding.control : 0) | (option ? KeyBinding.option : 0)
            | (command ? KeyBinding.command : 0)
    }

    func matches(keyCode: UInt16, shift: Bool, control: Bool, option: Bool, command: Bool) -> Bool {
        keyCode == self.keyCode
            && modifiers == KeyBinding.modifiers(shift: shift, control: control, option: option, command: command)
    }

    /// Stored form "keyCode:modifiers".
    var stored: String { "\(keyCode):\(modifiers)" }

    init?(stored: String) {
        let parts = stored.split(separator: ":")
        guard parts.count == 2, let code = UInt16(parts[0]), let mods = UInt8(parts[1]) else { return nil }
        self.init(code, mods)
    }

    static let names: [UInt16: String] = [
        122: "F1", 120: "F2", 99: "F3", 118: "F4", 96: "F5", 97: "F6", 98: "F7", 100: "F8", 101: "F9", 109: "F10",
        103: "F11", 111: "F12", 105: "F13", 107: "F14", 113: "F15", 36: "↩", 76: "⌤", 48: "⇥", 49: "Space", 51: "⌫",
        53: "Esc", 123: "←", 124: "→", 125: "↓", 126: "↑", 115: "Home", 119: "End", 116: "PgUp", 121: "PgDn",
    ]

    /// "⌃⌥⇧⌘F2", in the macOS modifier order.
    var label: String {
        var text = ""
        if modifiers & KeyBinding.control != 0 { text += "⌃" }
        if modifiers & KeyBinding.option != 0 { text += "⌥" }
        if modifiers & KeyBinding.shift != 0 { text += "⇧" }
        if modifiers & KeyBinding.command != 0 { text += "⌘" }
        if let name = KeyBinding.names[keyCode] { return text + name }
        if let (plain, _) = KeyMap.printable[keyCode] { return text + plain.uppercased() }
        return text + "#\(keyCode)"
    }
}

/// The commands a key binding can trigger, with their defaults (Windows app keys, Apple's hanja key).
enum Command: String, CaseIterable {
    case candidate, flip, pause, recent, hanja, switchKey, modeAuto, modeKorean, modeEnglish

    var defaultBinding: KeyBinding? {
        switch self {
        case .candidate: return KeyBinding(120)
        case .flip: return KeyBinding(120, KeyBinding.shift)
        case .pause: return KeyBinding(111)
        case .recent: return KeyBinding(120, KeyBinding.option)
        case .hanja: return KeyBinding(36, KeyBinding.option)
        case .switchKey, .modeAuto, .modeKorean, .modeEnglish: return nil
        }
    }
}

extension KeyMap {
    /// The command bound to the key, if any. `bindings` holds the user's choices (nil = unset); commands left out use their
    /// defaults.
    static func command(keyCode: UInt16, shift: Bool, control: Bool, option: Bool, command: Bool,
                        bindings: [Command: KeyBinding?]) -> Command? {
        Command.allCases.first { c in
            let binding = bindings[c] ?? c.defaultBinding
            return binding?.matches(keyCode: keyCode, shift: shift, control: control, option: option, command: command)
                ?? false
        }
    }

    /// A printable key's event carrying text another program inserted as Unicode (CGEventKeyboardSetUnicodeString), which
    /// must go in as it is (C-CTX-08). Conservative, so a layout quirk never stops ordinary typing: non-ASCII text on a
    /// key without ⌘⌃⌥, or key code 0 (where such tools put their text) carrying what the layout does not type there.
    /// `expected` is the current layout's character for the key with the same Shift and Caps Lock state, nil when unknown.
    static func isInjected(keyCode: UInt16, characters: String?, expected: String?) -> Bool {
        guard keyCode == space || printable[keyCode] != nil, let characters, !characters.isEmpty else { return false }
        if characters.unicodeScalars.contains(where: { $0.value > 0x7e }) { return true }
        if keyCode == 0, let expected { return characters != expected }
        return false
    }
}

/// The modifier keys that can be tapped alone as a switch key (M-03 · C-MOD-03 · M-18), by key code.
enum TapKey: UInt16, CaseIterable {
    case leftShift = 56, rightShift = 60, rightOption = 61, rightControl = 62, rightCommand = 54, function = 63

    /// NSEvent device-dependent bit that tells this side apart (fn uses the function flag).
    var deviceBit: UInt {
        switch self {
        case .leftShift: return 0x02
        case .rightShift: return 0x04
        case .rightOption: return 0x40
        case .rightControl: return 0x2000
        case .rightCommand: return 0x10
        case .function: return 0x80_0000
        }
    }
}

/// A modifier pressed and released alone within `window` (right ⌘ tap, M-03). A key or another modifier in between, or a
/// longer hold, makes it an ordinary modifier (⌘C, ⌘-drag).
struct ModifierTap {
    static let window: TimeInterval = 0.3
    private var downAt: TimeInterval?

    /// A modifier changed. `isTapKey`: the change is the tap key's; `down`: the tap key is now held; `others`: another
    /// modifier is held. Returns true on a completed tap.
    mutating func changed(isTapKey: Bool, down: Bool, others: Bool, at time: TimeInterval) -> Bool {
        if isTapKey && down {
            downAt = others ? nil : time
            return false
        }
        defer { downAt = nil }
        guard isTapKey, !others, let start = downAt else { return false }
        return time - start <= ModifierTap.window
    }

    mutating func keyDown() { downAt = nil }
    mutating func reset() { downAt = nil }
}

/// Caps Lock as a switch key (C-MOD-10): a short press switches; a long press (0.5 s) or Shift+Caps Lock locks capitals;
/// a press while capitals are locked only unlocks them. Mode changes leave the lock as it is.
struct CapsLockGesture {
    static let hold: TimeInterval = 0.5

    enum Outcome: Equatable {
        case none
        /// Switch language and turn the lock off again (the system turned it on at the press).
        case switchLanguage
        case lock
        case unlock
    }

    /// Whether capitals are locked, as far as the gesture knows.
    private(set) var locked = false
    private var pressedAt: TimeInterval?
    private var withShift = false
    private var whileLocked = false

    /// With Input Monitoring the key's press and release are seen exactly.
    mutating func press(at time: TimeInterval, shift: Bool) {
        pressedAt = time
        withShift = shift
        whileLocked = locked
    }

    mutating func release(at time: TimeInterval) -> Outcome {
        guard let start = pressedAt else { return .none }
        pressedAt = nil
        if whileLocked {
            locked = false
            return .unlock
        }
        if withShift || time - start >= CapsLockGesture.hold {
            locked = true
            return .lock
        }
        locked = false
        return .switchLanguage
    }

    /// Without the monitor only lock changes are visible: a lock turned on without Shift is a switch, Shift+Caps Lock
    /// keeps it (no long press).
    mutating func lockChanged(on: Bool, shift: Bool) -> Outcome {
        guard on else {
            locked = false
            return .none
        }
        locked = shift
        return shift ? .none : .switchLanguage
    }

    /// The actual lock state (read when the input method becomes active).
    mutating func sync(locked: Bool) {
        self.locked = locked
        pressedAt = nil
    }
}
