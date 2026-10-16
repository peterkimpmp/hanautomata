import Foundation
import CryptoKit

struct ProofreadingSelection: Equatable {
    let text: String
    let document: String
    let revision: String
    let range: NSRange
    static func == (lhs: Self, rhs: Self) -> Bool {
        lhs.text.utf8.elementsEqual(rhs.text.utf8) && lhs.document == rhs.document
            && lhs.revision == rhs.revision && lhs.range == rhs.range
    }
}

struct ProofreadingRequest: Codable {
    let request_id: String
    let document_id: String
    let revision: String
    let text: String
    var protected_words: [String] = []
}
struct ProofreadingReport: Decodable {
    struct Span: Decodable { let start: Int; let end: Int }
    struct Diagnostic: Decodable {
        let kind: String
        let utf8: Span
        let utf16: Span
        let original: String
        let suggestions: [String]
        let rule_id: String?
    }
    let schema_version: Int
    let request_id: String
    let document_id: String
    let revision: String
    let snapshot_sha256: String
    let limited: Bool
    let diagnostics: [Diagnostic]
    let checked_words: Int
}
enum ProofreadingContract {
    static func matches(_ request: ProofreadingRequest, _ report: ProofreadingReport) -> Bool {
        report.schema_version == 1 && request.request_id == report.request_id && request.document_id == report.document_id
            && request.revision == report.revision && report.snapshot_sha256 == SHA256.hash(data: Data(request.text.utf8)).map { String(format: "%02x", $0) }.joined()
    }
    static func preview(_ request: ProofreadingRequest, _ report: ProofreadingReport, diagnostic: Int, candidate: Int) -> String? {
        guard matches(request, report), report.diagnostics.indices.contains(diagnostic) else { return nil }
        let item = report.diagnostics[diagnostic]
        guard item.suggestions.indices.contains(candidate), item.utf16.start >= 0, item.utf16.end >= item.utf16.start,
              item.utf16.end <= request.text.utf16.count,
              let range = Range(NSRange(location: item.utf16.start, length: item.utf16.end - item.utf16.start), in: request.text),
              request.text[range].utf8.elementsEqual(item.original.utf8),
              request.text[..<range.lowerBound].utf8.count == item.utf8.start,
              request.text[..<range.upperBound].utf8.count == item.utf8.end,
              !request.protected_words.contains(where: { $0.precomposedStringWithCanonicalMapping == item.original.precomposedStringWithCanonicalMapping })
        else { return nil }
        let replacement = item.suggestions[candidate]
        guard !replacement.isEmpty, replacement.utf8.count <= 256,
              replacement.unicodeScalars.allSatisfy({ $0 == " " || (0xac00...0xd7a3).contains(Int($0.value)) }) else { return nil }
        var text = request.text
        text.replaceSubrange(range, with: replacement)
        return text
    }
}

/// Stdio only; no temporary text files, network, diagnostics, or persistent request cache.
final class ProofreadingWorker {
    private var process: Process?
    private var input: FileHandle?
    private let writer = DispatchQueue(label: "Hanautomata.proofread.write")
    private var generation = UUID()
    var receive: ((Data) -> Void)?
    var failed: (() -> Void)?

    func start() throws {
        stop()
        let resources = Bundle.main.resourceURL!.appendingPathComponent("proofreading")
        let executable = resources.appendingPathComponent("hanautomata-proofread")
        guard FileManager.default.isExecutableFile(atPath: executable.path) else {
            throw NSError(domain: "Proofreading", code: 1)
        }
        let task = Process(), incoming = Pipe(), outgoing = Pipe()
        task.executableURL = executable
        task.arguments = ["--dictionary", resources.appendingPathComponent("data").path, "--serve", "--rules"]
        task.standardInput = incoming; task.standardOutput = outgoing; task.standardError = FileHandle.nullDevice
        try task.run()
        process = task; input = incoming.fileHandleForWriting
        let generation = self.generation
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            var pending = Data()
            let reader = outgoing.fileHandleForReading
            while true {
                let bytes = reader.availableData
                if bytes.isEmpty { break }
                pending.append(bytes)
                if pending.count > 262_144 { break }
                while let end = pending.firstIndex(of: 10) {
                    let frame = pending[..<end]
                    pending.removeSubrange(...end)
                    DispatchQueue.main.async { [weak self] in
                        guard let self, self.generation == generation else { return }
                        self.receive?(Data(frame))
                    }
                }
            }
            DispatchQueue.main.async { [weak self] in
                guard let self, self.generation == generation else { return }
                self.stop(); self.failed?()
            }
        }
    }
    func send(_ value: [String: Any]) {
        guard let handle = input, var bytes = try? JSONSerialization.data(withJSONObject: value), bytes.count < 65_536 else { return }
        bytes.append(10)
        writer.async { try? handle.write(contentsOf: bytes) }
    }
    func check(_ request: ProofreadingRequest) {
        guard let bytes = try? JSONEncoder().encode(request), let value = try? JSONSerialization.jsonObject(with: bytes) else { return }
        send(["schema_version": 1, "command": "check", "request": value])
    }
    func cancel(_ id: String) { send(["schema_version": 1, "command": "cancel", "request_id": id]) }
    func stop() {
        generation = UUID()
        try? input?.close(); input = nil
        // EOF makes the supervisor kill/reap its native child. No synchronous wait on the UI thread.
        if let task = process { DispatchQueue.global().async { task.waitUntilExit() } }
        process = nil
    }
    deinit { try? input?.close() }
}
