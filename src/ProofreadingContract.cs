using System;
using System.Linq;
using System.Security.Cryptography;
using System.Text;

namespace Hanautomata
{
    // JSON names intentionally match the versioned Rust protocol. No dependency on UIA/WinForms.
    internal sealed class ProofRequest
    {
        public string request_id { get; set; }
        public string document_id { get; set; }
        public string revision { get; set; }
        public string text { get; set; }
        public string[] protected_words { get; set; }
    }
    internal sealed class ProofSpan { public int start { get; set; } public int end { get; set; } }
    internal sealed class ProofDiagnostic
    {
        public string kind { get; set; }
        public ProofSpan utf8 { get; set; }
        public ProofSpan utf16 { get; set; }
        public string original { get; set; }
        public string[] suggestions { get; set; }
        public string rule_id { get; set; }
    }
    internal sealed class ProofReport
    {
        public int schema_version { get; set; }
        public string request_id { get; set; }
        public string document_id { get; set; }
        public string revision { get; set; }
        public string snapshot_sha256 { get; set; }
        public bool limited { get; set; }
        public int checked_words { get; set; }
        public ProofDiagnostic[] diagnostics { get; set; }
    }
    internal sealed class ProofReply
    {
        public int schema_version { get; set; }
        public string @event { get; set; }
        public string request_id { get; set; }
        public string code { get; set; }
        public ProofReport report { get; set; }
    }
    internal static class ProofreadingContract
    {
        internal static string Hash(string text)
        { using (var sha = SHA256.Create()) return BitConverter.ToString(sha.ComputeHash(Encoding.UTF8.GetBytes(text))).Replace("-", "").ToLowerInvariant(); }
        internal static bool Matches(ProofRequest request, ProofReport report)
        {
            return request != null && report != null && request.text != null && report.schema_version == 1
                && request.request_id == report.request_id && request.document_id == report.document_id
                && request.revision == report.revision && Hash(request.text) == report.snapshot_sha256;
        }
        internal static string Preview(ProofRequest request, ProofReport report, int diagnostic, int candidate)
        {
            if (!Matches(request, report) || report.diagnostics == null || diagnostic < 0 || diagnostic >= report.diagnostics.Length) return null;
            var d = report.diagnostics[diagnostic];
            if (d == null || d.utf16 == null || d.utf8 == null || d.suggestions == null || candidate < 0 || candidate >= d.suggestions.Length) return null;
            int start = d.utf16.start, end = d.utf16.end;
            if (start < 0 || end < start || end > request.text.Length || !Boundary(request.text, start) || !Boundary(request.text, end)
                || request.text.Substring(start, end - start) != d.original
                || Encoding.UTF8.GetByteCount(request.text.Substring(0, start)) != d.utf8.start
                || Encoding.UTF8.GetByteCount(request.text.Substring(0, end)) != d.utf8.end
                || (request.protected_words ?? new string[0]).Any(w => w.Normalize() == d.original.Normalize())) return null;
            string replacement = d.suggestions[candidate];
            if (string.IsNullOrEmpty(replacement) || Encoding.UTF8.GetByteCount(replacement) > 256
                || replacement.Any(c => c != ' ' && (c < '\uac00' || c > '\ud7a3'))) return null;
            return request.text.Substring(0, start) + replacement + request.text.Substring(end);
        }
        static bool Boundary(string text, int offset)
        { return offset == 0 || offset == text.Length || !char.IsLowSurrogate(text[offset]); }
    }
}
