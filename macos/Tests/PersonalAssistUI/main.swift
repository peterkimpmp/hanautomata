// Same-process native controls + a real Rust worker. Not an IMK / physical typing acceptance test.
import Cocoa

_ = NSApplication.shared
let directory = FileManager.default.temporaryDirectory.appendingPathComponent("hanautomata-assist-ui-" + UUID().uuidString)
try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
defer { try? FileManager.default.removeItem(at: directory) }
let store = directory.appendingPathComponent("personal-expressions.json")
let service = PersonalAssistClient(executable: URL(fileURLWithPath: CommandLine.arguments[1]), store: store)
var copied = ""
let panel = PersonalAssistPanel(service: service, copyText: { copied = $0; return true })
defer { panel.window.close() }
let root = panel.window.contentView!
func descendants(_ view: NSView) -> [NSView] { [view] + view.subviews.flatMap { descendants($0) } }
func button(_ title: String) -> NSButton { descendants(root).compactMap { $0 as? NSButton }.first { $0.title == title }! }
func field(_ name: String) -> NSTextField { descendants(root).compactMap { $0 as? NSTextField }.first { $0.accessibilityIdentifier() == name }! }
let table = descendants(root).compactMap { $0 as? NSTableView }.first!
let add = button("등록"), toggle = button("추천 사용 · 복사 선택을 순위에 반영"), undo = button("최근 순위 기억 되돌리기")
var checks = 0
func check(_ condition: Bool) { precondition(condition, "UI assertion \(checks + 1)"); checks += 1 }
func wait(_ condition: () -> Bool) {
    let end = Date().addingTimeInterval(5)
    while !condition() && Date() < end { RunLoop.main.run(until: Date().addingTimeInterval(0.01)) }
    check(condition())
}
wait { add.isEnabled }; check(table.numberOfRows == 0 && toggle.state == .off)
field("assist-expression").stringValue = "검토 후 회신드리겠습니다."
add.performClick(nil); wait { add.isEnabled }; check(table.numberOfRows == 1)
toggle.performClick(nil); wait { add.isEnabled }; check(toggle.state == .on)
let search = field("assist-search")
search.stringValue = "검토"; panel.controlTextDidChange(Notification(name: NSControl.textDidChangeNotification, object: search))
wait { table.numberOfRows == 1 && add.isEnabled }
button("복사").performClick(nil); wait { undo.isEnabled }; check(copied == "검토 후 회신드리겠습니다.")
undo.performClick(nil); wait { add.isEnabled }; check(!undo.isEnabled)
let saved = try JSONSerialization.jsonObject(with: Data(contentsOf: store)) as! [String: Any]
check((saved["entries"] as! [[String: Any]])[0]["copies"] as? Int == 0)
// Rapid edits must invalidate the visible rows immediately and only show the last search's result.
search.stringValue = "없는"; panel.controlTextDidChange(Notification(name: NSControl.textDidChangeNotification, object: search))
check(table.numberOfRows == 0 && !button("복사").isEnabled)
search.stringValue = "검토"; panel.controlTextDidChange(Notification(name: NSControl.textDidChangeNotification, object: search))
wait { table.numberOfRows == 1 && add.isEnabled }
root.layoutSubtreeIfNeeded(); root.displayIfNeeded()
if CommandLine.arguments.count > 2, let bitmap = root.bitmapImageRepForCachingDisplay(in: root.bounds) {
    root.cacheDisplay(in: root.bounds, to: bitmap)
    try bitmap.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: CommandLine.arguments[2]))
}
button("삭제").performClick(nil); wait { add.isEnabled && table.numberOfRows == 0 }
print("PASS: \(checks) native personal-assist UI assertions; clipboard replaced with a test sink")
