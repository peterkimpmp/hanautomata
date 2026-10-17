import Foundation

struct AssistItem: Codable {
    let id: String
    let text: String
    let copies: UInt32
    let last_copied_day: UInt64
}
struct AssistReply: Codable {
    let schema_version: Int
    let request_id: String
    let ok: Bool
    let enabled: Bool
    let items: [AssistItem]
    let error: String
    let undo_id: String
}
enum AssistError: Error { case connection, invalidReply }

/// Serial process I/O on a worker queue; no text, model, or file work on the input callback.
final class PersonalAssistClient {
    private let executable: URL
    private let store: URL
    private let queue = DispatchQueue(label: "Hanautomata.personal-expressions")
    private let lifetime = NSLock()
    private var process: Process?
    private var closed = false
    private var sequence = 0
    private var activeRequest = ""

    init(executable: URL, store: URL) { self.executable = executable; self.store = store }

    func send(_ command: String, fields: [String: Any] = [:], completion: @escaping (Result<AssistReply, Error>) -> Void) {
        lifetime.lock()
        if closed { lifetime.unlock(); return }
        sequence += 1; let id = String(sequence)
        lifetime.unlock()
        queue.async { [self] in
            let result: Result<AssistReply, Error>
            do {
                lifetime.lock()
                if closed { lifetime.unlock(); return }
                lifetime.unlock()
                let child = try worker()
                lifetime.lock(); activeRequest = id; lifetime.unlock()
                let timeout = DispatchWorkItem { [weak self, weak child] in
                    guard let self, let child else { return }
                    self.lifetime.lock(); defer { self.lifetime.unlock() }
                    if self.activeRequest == id, child.isRunning { child.terminate() }
                }
                DispatchQueue.global().asyncAfter(deadline: .now() + 3, execute: timeout)
                defer {
                    lifetime.lock(); activeRequest = ""; lifetime.unlock()
                    timeout.cancel()
                }
                var request = fields
                request["schema_version"] = 1; request["request_id"] = id; request["command"] = command
                var data = try JSONSerialization.data(withJSONObject: request); data.append(10)
                guard data.count <= 16_384, let input = child.standardInput as? Pipe,
                      let output = child.standardOutput as? Pipe else { throw AssistError.connection }
                try input.fileHandleForWriting.write(contentsOf: data)
                let bytes = try Self.readFrame(output.fileHandleForReading)
                let reply = try JSONDecoder().decode(AssistReply.self, from: bytes)
                guard reply.schema_version == 1, reply.request_id == id || (reply.request_id.isEmpty && !reply.ok),
                      reply.items.count <= 512, reply.items.allSatisfy({ $0.id.count == 64 && !$0.text.isEmpty && $0.text.unicodeScalars.count <= 256 })
                else { throw AssistError.invalidReply }
                result = .success(reply)
            } catch {
                lifetime.lock()
                if let process, process.isRunning { process.terminate() }
                process = nil
                lifetime.unlock()
                result = .failure(AssistError.connection)
            }
            lifetime.lock(); let deliver = !closed; lifetime.unlock()
            if deliver { DispatchQueue.main.async { completion(result) } }
        }
    }

    private func worker() throws -> Process {
        lifetime.lock(); defer { lifetime.unlock() }
        guard !closed else { throw AssistError.connection }
        if let process, process.isRunning { return process }
        let child = Process()
        child.executableURL = executable; child.arguments = ["--store", store.path]
        child.standardInput = Pipe(); child.standardOutput = Pipe()
        child.standardError = FileHandle.nullDevice
        try child.run(); process = child
        return child
    }

    static func readFrame(_ handle: FileHandle) throws -> Data {
        var data = Data()
        // read(upToCount:) may wait to fill its count on a pipe. availableData returns the current pipe chunk, so a
        // short JSON line is delivered without waiting for the next request or killing the worker at its deadline.
        while true {
            let chunk = handle.availableData
            if chunk.isEmpty { throw AssistError.connection }
            data.append(chunk)
            guard data.count <= 1_048_576 else { throw AssistError.invalidReply }
            if let end = data.firstIndex(of: 10) {
                guard end == data.index(before: data.endIndex) else { throw AssistError.invalidReply }
                return data.prefix(upTo: end)
            }
        }
    }

    func close() {
        lifetime.lock(); defer { lifetime.unlock() }
        closed = true
        if let process, process.isRunning { process.terminate() }
    }

    static func message(_ code: String) -> String {
        switch code {
        case "invalid_text": return "줄바꿈·제어문자 없이 2~256자로 등록해 주세요."
        case "full": return "512개까지 저장할 수 있습니다. 사용하지 않는 표현을 삭제해 주세요."
        case "disabled": return "추천 사용을 켜면 순위를 기억합니다."
        case "stale_undo": return "되돌릴 수 있는 최근 순위 이력이 없습니다."
        case "not_found": return "표현이 삭제되었습니다. 다시 불러와 주세요."
        case "store_unavailable": return "저장소를 열거나 저장하지 못했습니다. 다른 내 표현 창을 닫고 재시도해 주세요. 손상 파일은 덮어쓰지 않습니다."
        default: return "추천 서비스에 연결하지 못했습니다. 서비스가 포함된 빌드에서 다시 시도해 주세요."
        }
    }
}
