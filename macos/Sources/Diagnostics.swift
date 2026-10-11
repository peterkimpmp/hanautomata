import Foundation

/// Content-free counters (C-OPS-02): how keys were handled, how words were committed and decided, corrections, pass-through
/// reasons per app and key latency (C-OPS-05). No key, character or word is ever recorded; apps appear as bundle IDs.
/// The input method saves them to diagnostics.json next to the learning file; `Hanautomata --diagnose` prints them.
final class Diagnostics {
    struct App: Codable, Equatable {
        var keys = 0
        var passed: [String: Int] = [:]
    }

    /// Key latency in microsecond buckets: from the shell receiving the key event to returning its result.
    struct Latency: Codable, Equatable {
        static let bounds = [10, 20, 50, 100, 200, 500, 1_000, 2_000, 5_000, 10_000, 20_000, 50_000]
        var buckets = [Int](repeating: 0, count: Latency.bounds.count + 1)
        var count = 0
        var maxMicros = 0

        mutating func record(micros: Int) {
            let index = Latency.bounds.firstIndex { micros <= $0 } ?? Latency.bounds.count
            buckets[index] += 1
            count += 1
            maxMicros = max(maxMicros, micros)
        }

        /// The bucket bound that `fraction` of the samples stay under (nil without samples; the maximum for the last
        /// bucket).
        func percentile(_ fraction: Double) -> Int? {
            guard count > 0 else { return nil }
            let target = Int((Double(count) * fraction).rounded(.up))
            var seen = 0
            for (index, n) in buckets.enumerated() {
                seen += n
                if seen >= target { return index < Latency.bounds.count ? Latency.bounds[index] : maxMicros }
            }
            return maxMicros
        }
    }

    struct Snapshot: Codable, Equatable {
        /// Whole seconds, as the file stores them.
        var since = Date(timeIntervalSince1970: Date().timeIntervalSince1970.rounded(.down))
        var keys = 0
        /// Keys passed untouched, by reason: excluded · paused · injected.
        var passed: [String: Int] = [:]
        /// Words committed, by what committed them: space · enter · tab · navigation · shortcut · switch · bound · other ·
        /// focus · failure.
        var commits: [String: Int] = [:]
        /// Committed words by decision strength and side, e.g. "Pattern ko", "Uncertain en", "fixed ko".
        var decisions: [String: Int] = [:]
        /// candidate · switch · reconvert · reconvertFailed · flip · flipFailed.
        var corrections: [String: Int] = [:]
        /// Key event sources: hid (the keyboard) · other (another program) — C-CTX-08 evidence.
        var sources: [String: Int] = [:]
        /// Keys typed while a secure field had focus (learning suspended, C-CTX-01).
        var secureKeys = 0
        var failures = 0
        var latency = Latency()
        var apps: [String: App] = [:]
    }

    private(set) var snapshot = Snapshot()
    /// Something changed since the last save.
    private(set) var dirty = false

    init(_ snapshot: Snapshot = Snapshot()) {
        self.snapshot = snapshot
    }

    private func bump(_ table: inout [String: Int], _ key: String) {
        table[key, default: 0] += 1
        dirty = true
    }

    func key(app: String) {
        snapshot.keys += 1
        snapshot.apps[app.isEmpty ? "?" : app, default: App()].keys += 1
        dirty = true
    }

    func passed(_ reason: String, app: String) {
        bump(&snapshot.passed, reason)
        bump(&snapshot.apps[app.isEmpty ? "?" : app, default: App()].passed, reason)
    }

    func commit(_ path: String) { bump(&snapshot.commits, path) }
    func decision(_ confidence: String, korean: Bool) {
        bump(&snapshot.decisions, (confidence.isEmpty ? "fixed" : confidence) + (korean ? " ko" : " en"))
    }
    func correction(_ kind: String) { bump(&snapshot.corrections, kind) }
    func source(hardware: Bool) { bump(&snapshot.sources, hardware ? "hid" : "other") }

    func secureKey() {
        snapshot.secureKeys += 1
        dirty = true
    }

    func failure() {
        snapshot.failures += 1
        dirty = true
    }

    func latency(nanoseconds: UInt64) {
        snapshot.latency.record(micros: Int(nanoseconds / 1_000))
        dirty = true
    }

    /// Continues from counters saved by an earlier run.
    func restore(_ saved: Snapshot) {
        snapshot = saved
        dirty = false
    }

    func reset() {
        snapshot = Snapshot()
        dirty = true
    }

    /// Writes the counters atomically and clears `dirty`.
    func save(to url: URL) throws {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        encoder.dateEncodingStrategy = .iso8601
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try encoder.encode(snapshot).write(to: url, options: .atomic)
        dirty = false
    }

    static func load(from url: URL) -> Snapshot? {
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        return (try? Data(contentsOf: url)).flatMap { try? decoder.decode(Snapshot.self, from: $0) }
    }

    /// A readable summary for `--diagnose`.
    static func report(_ s: Snapshot) -> String {
        func table(_ t: [String: Int]) -> String {
            t.isEmpty ? "-" : t.sorted { $0.value > $1.value }.map { "\($0.key) \($0.value)" }.joined(separator: " · ")
        }
        func ms(_ micros: Int?) -> String {
            guard let micros else { return "-" }
            return String(format: "%.2fms", Double(micros) / 1_000)
        }
        let formatter = ISO8601DateFormatter()
        var lines = [
            "Hanautomata diagnostics since \(formatter.string(from: s.since)) — counts only, no typed text",
            "keys \(s.keys) · passed: \(table(s.passed)) · secure-field keys \(s.secureKeys) · engine failures \(s.failures)",
            "latency (\(s.latency.count) keys): p50 ≤ \(ms(s.latency.percentile(0.5))) · p99 ≤ \(ms(s.latency.percentile(0.99)))"
                + " · max \(ms(s.latency.count > 0 ? s.latency.maxMicros : nil))",
            "commits: \(table(s.commits))",
            "decisions: \(table(s.decisions))",
            "corrections: \(table(s.corrections))",
            "event sources: \(table(s.sources))",
            "apps:",
        ]
        for (id, app) in s.apps.sorted(by: { $0.value.keys > $1.value.keys }) {
            lines.append("  \(id): keys \(app.keys)" + (app.passed.isEmpty ? "" : " · passed: \(table(app.passed))"))
        }
        return lines.joined(separator: "\n")
    }
}
