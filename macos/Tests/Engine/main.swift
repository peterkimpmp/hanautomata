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
    var last = KeyResult(commit: "", marked: "", handled: false, alternative: "", uncertain: false, reason: "", label: "",
                         confidence: "", korean: false, learned: false, failed: false)
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
check(engine.correctSpacing(text: "😀 할수있다\t하는것") == "😀 할 수 있다\t하는 것", "spacing bridge preserves Unicode and whitespace")
check(engine.correctSpacing(text: "설거지 그럴듯") == "설거지 그럴듯", "spacing bridge preserves counterexamples")
// F2 after a Space commit (C-COR-02) and the candidate window's evidence label.
let fresh = Engine(home: "/Users/홍길동", storePath: dir.appendingPathComponent("fresh.json").path)
var last = KeyResult(commit: "", marked: "", handled: false, alternative: "", uncertain: false, reason: "", label: "",
                     confidence: "", korean: false, learned: false, failed: false)
for c in "rm" { last = fresh.handle(kind: .text, raw: String(c), key: String(c)) }
check(last.label == "확인 필요", "uncertain words carry the 확인 필요 label")
fresh.cancel()
for c in "dkssud" { _ = fresh.handle(kind: .text, raw: String(c), key: String(c)) }
_ = fresh.handle(kind: .space, raw: "", key: "")
let swap = fresh.reconversion()
check(swap?.old == "안녕 " && swap?.new == "dkssud ", "F2 after Space offers the swap")
let swapped = fresh.reconvert()
check(swapped.handled && swapped.learned && fresh.learnedCount() == 1, "reconvert learns")
check(fresh.reconversion()?.new == "안녕 ", "and can swap back")
fresh.forgetReconversion()
check(fresh.reconversion() == nil, "forget")
try? FileManager.default.removeItem(at: dir)

// 1.0 surface: layouts, exception lists, learned words, bundle, hanja (C-LAY-02 · C-LEX-02/03/04/06).
let more = Engine(home: "/Users/홍길동", storePath: dir.appendingPathComponent("more.json").path)
_ = more.setKeyboard(kind: .sebeolsik390)
var r = more.handle(kind: .text, raw: "k", key: "k")
for c in "fs" { r = more.handle(kind: .text, raw: String(c), key: String(c)) }
check(r.marked == "간" && more.keyboard() == .sebeolsik390, "Sebeolsik 390 through the bridge")
check(more.setKeyboard(kind: .dubeolsik).commit == "간", "changing the layout commits")
more.setExceptions(keep: ["dkssud"], convert: ["그"])
check(more.keepList() == ["dkssud"] && more.convertList() == ["그"], "exception lists round-trip")
check(more.parseList(text: "# c\na\n\n b \n") == ["a", "b"], "list text form")
for c in "rm" { r = more.handle(kind: .text, raw: String(c), key: String(c)) }
check(r.marked == "그", "an always-convert entry applies")
_ = more.handle(kind: .candidate, raw: "", key: "")
_ = more.handle(kind: .space, raw: "", key: "")
check(more.learnedWords().first?.raw == "rm" && more.learnedWords().first?.korean == false, "learned words list")
let bundle = more.exportBundle(settings: "{\"mode\":\"auto\"}")
check(more.forgetWord(raw: "rm", keys: "rm") >= 1 && more.learnedCount() == 0, "one word forgotten")
let back = more.importBundle(text: bundle)
check(back.error.isEmpty && back.words >= 1 && back.settings.contains("auto") && more.learnedCount() >= 1, "bundle import")
check(more.importBundle(text: "{}").error.isEmpty == false, "a broken bundle reports an error")
check(more.hanja(syllable: "한", extended: false).first == "寒" && more.hanja(syllable: "ab", extended: true).isEmpty, "hanja")
more.setLearningPaused(paused: true)
check(more.learningPaused(), "learning pause flag")

if failures.isEmpty { print("PASS: \(count) engine bridge assertions") } else {
    print("FAIL: \(failures.count) of \(count)"); for f in failures { print("  - \(f)") }; exit(1)
}
