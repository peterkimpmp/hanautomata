// Real NSTextView storage and marked-text semantics. This is not an IMK event-dispatch / physical keyboard test.
import Cocoa

final class NativeClient: TextClient {
    let view: NSTextView
    init(_ view: NSTextView) { self.view = view }
    var identity: ObjectIdentifier { ObjectIdentifier(view) }
    var bundleIdentifier: String { "org.hanautomata.test.native" }
    func insert(_ text: String, replacing range: NSRange?) {
        view.insertText(text, replacementRange: range ?? NSRange(location: NSNotFound, length: 0))
    }
    func setMarked(_ text: String, uncertain: Bool) {
        view.setMarkedText(text, selectedRange: NSRange(location: text.utf16.count, length: 0),
                           replacementRange: NSRange(location: NSNotFound, length: 0))
    }
    func selectedRange() -> NSRange? { view.selectedRange() }
    func text(in range: NSRange) -> String? {
        guard NSMaxRange(range) <= view.string.utf16.count else { return nil }
        return (view.string as NSString).substring(with: range)
    }
    func caretRect() -> CGRect? { nil }
    func selectMode(_ id: String) -> Bool { false }
}

let suite = "hanautomata-appkit-tests-\(ProcessInfo.processInfo.processIdentifier)"
Settings.defaults = UserDefaults(suiteName: suite)!
defer { Settings.defaults.removePersistentDomain(forName: suite) }
_ = NSApplication.shared
let storeDir = FileManager.default.temporaryDirectory.appendingPathComponent(suite)
defer { try? FileManager.default.removeItem(at: storeDir) }
let engine = Engine(home: storeDir.path, storePath: storeDir.appendingPathComponent("words.json").path)
let core = InputCore(engine: engine)
let view = NSTextView(frame: NSRect(x: 0, y: 0, width: 500, height: 100))
core.activate(NativeClient(view), capsLocked: false)
let codes = Dictionary(uniqueKeysWithValues: KeyMap.printable.map { ($0.value.0, $0.key) })
var assertions = 0
func check(_ value: Bool, _ label: String) {
    assertions += 1
    if !value { print("FAIL: \(label): \(view.string.debugDescription)"); exit(1) }
}
func key(_ code: UInt16, _ modifiers: Modifiers = []) {
    // IMK creates an adapter for each event. Identity must follow the native client, not this temporary wrapper.
    _ = core.keyDown(keyCode: code, modifiers: modifiers, isRepeat: false, client: NativeClient(view))
}
view.insertText("😀 ", replacementRange: NSRange(location: NSNotFound, length: 0))
for c in "gkftndlTek" { key(codes[Character(c.lowercased())]!, c.isUppercase ? [.shift] : []) }
check(view.hasMarkedText() && view.string == "😀 할수있다", "native marked text remains unspaced")
key(49)
check(!view.hasMarkedText() && view.string == "😀 할 수 있다 ", "native commit replaces the composition once")
check(view.selectedRange() == NSRange(location: view.string.utf16.count, length: 0), "UTF-16 caret after emoji")
key(120)
check(view.string == "😀 할수있다 " && engine.learnedCount() == 0, "new event adapter restores original without learning")
key(120)
check(view.string == "😀 gkftndlTek " && engine.learnedCount() == 1, "second F2 uses the ordinary language alternative")
print("PASS: \(assertions) native AppKit assertions")
