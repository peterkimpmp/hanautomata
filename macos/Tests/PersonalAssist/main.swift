import Foundation

let directory = FileManager.default.temporaryDirectory.appendingPathComponent("hanautomata-assist-swift-" + UUID().uuidString)
try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
defer { try? FileManager.default.removeItem(at: directory) }
let client = PersonalAssistClient(executable: URL(fileURLWithPath: CommandLine.arguments[1]),
    store: directory.appendingPathComponent("personal-expressions.json"))
defer { client.close() }
var checks = 0
func check(_ value: Bool) { precondition(value, "assist assertion \(checks + 1)"); checks += 1 }
func ask(_ command: String, _ fields: [String: Any] = [:]) -> AssistReply {
    var response: Result<AssistReply, Error>?
    client.send(command, fields: fields) { response = $0 }
    let end = Date().addingTimeInterval(6)
    while response == nil && Date() < end { RunLoop.main.run(until: Date().addingTimeInterval(0.01)) }
    guard let response, case .success(let reply) = response else { fatalError("assist response failed") }
    return reply
}
var reply = ask("list"); check(reply.ok && !reply.enabled && reply.items.isEmpty)
check(ask("add", ["text": "검토 후 회신드리겠습니다."]).ok)
check(ask("set_enabled", ["enabled": true]).enabled)
reply = ask("query", ["text": "검토"]); check(reply.items.count == 1)
let id = reply.items[0].id
reply = ask("copied", ["entry_id": id]); check(reply.ok && !reply.undo_id.isEmpty)
let undone = ask("undo", ["undo_id": reply.undo_id])
check(undone.ok && undone.items.count == 1 && undone.items[0].copies == 0)
check(!ask("unknown").ok)
check(ask("list").items.count == 1)
check(ask("clear").items.isEmpty)
let broken = directory.appendingPathComponent("broken-frame")
try Data("partial".utf8).write(to: broken)
let handle = try FileHandle(forReadingFrom: broken)
do { _ = try PersonalAssistClient.readFrame(handle); fatalError("partial reply accepted") } catch { checks += 1 }
try handle.close()
print("PASS: \(checks) Swift personal-assist client assertions")
