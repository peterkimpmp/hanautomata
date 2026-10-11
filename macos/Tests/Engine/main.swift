// Swift ↔ Rust smoke test through the generated UniFFI bindings (no InputMethodKit). Built and run by build.sh --test.
import Foundation

var count = 0
var failures: [String] = []
func check(_ condition: Bool, _ label: String) {
    count += 1
    if !condition { failures.append(label) }
}

let dir = FileManager.default.temporaryDirectory.appendingPathComponent("hanautomata-swift-\(ProcessInfo.processInfo.processIdentifier)")
try? FileManager.default.removeItem(at: dir)
let store = dir.appendingPathComponent("words.json").path
let engine = Engine(home: "/Users/홍길동", storePath: store)
check(engine.storeError() == "", "missing learning file starts empty")
func type(_ text: String) -> KeyResult {
    var last = KeyResult(commit: "", marked: "", handled: false, alternative: "", uncertain: false, reason: "", learned: false, failed: false)
    for c in text { last = engine.handle(kind: .text, raw: String(c), key: String(c)) }
    return last
}
check(type("dkssud").marked == "안녕", "composition shows Hangul")
let space = engine.handle(kind: .space, raw: "", key: "")
check(space.commit == "안녕 " && space.handled, "Space commits with a space")
// After a Korean word the near-tie "rm" leans Korean (continuity); cancel drops that context first.
let continued = type("rm")
check(continued.marked == "그", "continuity converts the near-tie after a Korean word")
engine.cancel()
let near = type("rm")
check(near.marked == "rm" && near.uncertain && near.alternative == "그", "near-tie is uncertain with the Hangul alternative")
let switched = engine.handle(kind: .switch, raw: "", key: "")
check(switched.marked == "그" && switched.handled, "switch flips the word")
let commit = engine.handle(kind: .enter, raw: "", key: "")
check(commit.commit == "그" && !commit.handled && commit.learned, "Enter commits, passes, and reports learning")
check(engine.save() == "" && FileManager.default.fileExists(atPath: store), "learning saved")
let reloaded = Engine(home: "/Users/홍길동", storePath: store)
_ = reloaded.handle(kind: .text, raw: "r", key: "r")
check(reloaded.handle(kind: .text, raw: "m", key: "m").marked == "그", "a restarted engine remembers the choice")
check(engine.handle(kind: .text, raw: "ab", key: "ab").commit == "" , "multi-character text is not a key")
check(engine.handle(kind: .text, raw: "가", key: "가").handled == false, "non-ASCII text is not composed")
_ = engine.setMode(mode: .english)
check(!engine.handle(kind: .text, raw: "a", key: "a").handled && engine.mode() == .english, "English mode passes")
check(engine.setSensitivity(name: "aggressive") == "aggressive" && engine.setSensitivity(name: "x") == "balanced", "sensitivity")
check(engine.flip(text: "dkssud 한글") == "안녕 gksrmf", "flip")
try? FileManager.default.removeItem(at: dir)

if failures.isEmpty { print("PASS: \(count) engine bridge assertions") } else {
    print("FAIL: \(failures.count) of \(count)"); for f in failures { print("  - \(f)") }; exit(1)
}
