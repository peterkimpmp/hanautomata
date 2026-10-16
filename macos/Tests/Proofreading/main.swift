import Foundation
import CryptoKit
let text = "😀 됬다"
let request = ProofreadingRequest(request_id: "r", document_id: "d", revision: "1", text: text)
let hash = SHA256.hash(data: Data(text.utf8)).map { String(format: "%02x", $0) }.joined()
let json: [String: Any] = ["schema_version": 1, "request_id": "r", "document_id": "d", "revision": "1",
    "snapshot_sha256": hash, "limited": false, "checked_words": 1,
    "diagnostics": [["kind": "unknown_word", "utf8": ["start": 5, "end": 11], "utf16": ["start": 3, "end": 5], "original": "됬다", "suggestions": ["됐다"]]]]
func report(_ value: [String: Any]) -> ProofreadingReport {
    try! JSONDecoder().decode(ProofreadingReport.self, from: JSONSerialization.data(withJSONObject: value))
}
assert(ProofreadingContract.preview(request, report(json), diagnostic: 0, candidate: 0) == "😀 됐다")
for key in ["request_id", "document_id", "revision", "snapshot_sha256"] {
    var stale = json; stale[key] = "different"
    assert(ProofreadingContract.preview(request, report(stale), diagnostic: 0, candidate: 0) == nil)
}
var protected = request; protected.protected_words = ["됬다"]
assert(ProofreadingContract.preview(protected, report(json), diagnostic: 0, candidate: 0) == nil)
assert(ProofreadingContract.preview(request, report(json), diagnostic: 7, candidate: 0) == nil)
assert(ProofreadingContract.preview(request, report(json), diagnostic: 0, candidate: 9) == nil)
var wrongRange = json
wrongRange["diagnostics"] = [["kind":"unknown_word", "utf8":["start":5,"end":11], "utf16":["start":1,"end":5], "original":"됬다", "suggestions":["됐다"]]]
assert(ProofreadingContract.preview(request, report(wrongRange), diagnostic: 0, candidate: 0) == nil)
let decomposed = "됬다".decomposedStringWithCanonicalMapping
let nfdRequest = ProofreadingRequest(request_id: "r", document_id: "d", revision: "1", text: decomposed)
var canonicalButDifferent = json
canonicalButDifferent["snapshot_sha256"] = SHA256.hash(data: Data(decomposed.utf8)).map { String(format: "%02x", $0) }.joined()
canonicalButDifferent["diagnostics"] = [["kind":"unknown_word", "utf8":["start":0,"end":decomposed.utf8.count],
    "utf16":["start":0,"end":decomposed.utf16.count], "original":"됬다", "suggestions":["됐다"]]]
assert(ProofreadingContract.preview(nfdRequest, report(canonicalButDifferent), diagnostic: 0, candidate: 0) == nil,
    "diagnostic original must match source bytes, not merely Swift canonical equality")
let first = "가" + "나".decomposedStringWithCanonicalMapping
let second = "가".decomposedStringWithCanonicalMapping + "나"
assert(first == second && first.utf16.count == second.utf16.count)
assert(ProofreadingSelection(text: first, document: "d", revision: "1", range: NSRange(location: 0, length: first.utf16.count))
    != ProofreadingSelection(text: second, document: "d", revision: "1", range: NSRange(location: 0, length: second.utf16.count)),
    "equal-length canonical variants are still changed source snapshots")
print("proofreading snapshot contract: 12 checks passed")
