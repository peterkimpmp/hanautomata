// Physical-key mapping and the ⇧Space switch gesture, kept free of AppKit so Tests/KeyMapTests.swift can run them.
// Characters follow the Windows app's Native.TryAscii on a US/Korean physical layout: the literal character applies
// Shift and Caps Lock, the Dubeolsik key applies Shift only (Caps Lock must not turn ㄱ into ㄲ).

enum KeyAction: Equatable {
    case text(raw: Character, key: Character)
    case space
    case switchKey
    case candidate
    case backspace
    case escape
    case enter
    case tab
    case navigation
    case shortcut
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

    static func action(keyCode: UInt16, shift: Bool, capsLock: Bool, command: Bool, control: Bool, option: Bool,
                       isRepeat: Bool, shiftSpaceEnabled: Bool, detector: inout ShiftSpaceDetector) -> KeyAction {
        let isSpace = keyCode == space
        let gesture = detector.keyDown(isSpace: isSpace)
        if command || control || option { return .shortcut }
        if isSpace {
            if gesture && shiftSpaceEnabled { return isRepeat ? .ignore : .switchKey }
            return .space
        }
        switch keyCode {
        case 36, 76: return .enter
        case 48: return .tab
        case 51: return .backspace
        case 53: return .escape
        case 120: return .candidate
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
