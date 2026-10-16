using System;
using Hanautomata;
static class Program
{
    static int checks;
    static void Check(bool ok) { checks++; if (!ok) throw new Exception("proofreading contract failed at " + checks); }
    static void Main()
    {
        var request = new ProofRequest { request_id="r", document_id="d", revision="1", text="😀 됬다", protected_words=new string[0] };
        var diagnostic = new ProofDiagnostic { original="됬다", utf8=new ProofSpan { start=5, end=11 }, utf16=new ProofSpan { start=3, end=5 }, suggestions=new[] {"됐다"} };
        var report = new ProofReport { schema_version=1, request_id="r", document_id="d", revision="1", snapshot_sha256=ProofreadingContract.Hash(request.text), diagnostics=new[] {diagnostic} };
        Check(ProofreadingContract.Preview(request, report, 0, 0) == "😀 됐다");
        report.request_id="old"; Check(ProofreadingContract.Preview(request, report, 0, 0) == null); report.request_id="r";
        report.document_id="other"; Check(ProofreadingContract.Preview(request, report, 0, 0) == null); report.document_id="d";
        report.revision="2"; Check(ProofreadingContract.Preview(request, report, 0, 0) == null); report.revision="1";
        report.snapshot_sha256="bad"; Check(ProofreadingContract.Preview(request, report, 0, 0) == null); report.snapshot_sha256=ProofreadingContract.Hash(request.text);
        diagnostic.utf16.start=1; Check(ProofreadingContract.Preview(request, report, 0, 0) == null); diagnostic.utf16.start=3;
        diagnostic.utf8.start=4; Check(ProofreadingContract.Preview(request, report, 0, 0) == null); diagnostic.utf8.start=5;
        request.protected_words=new[] {"됬다"}; Check(ProofreadingContract.Preview(request, report, 0, 0) == null); request.protected_words=new string[0];
        Check(ProofreadingContract.Preview(request, report, -1, 0) == null);
        Check(ProofreadingContract.Preview(request, report, 0, 9) == null);
        diagnostic.suggestions=new[] {"<script>"}; Check(ProofreadingContract.Preview(request, report, 0, 0) == null);
        Console.WriteLine("proofreading C# contract: " + checks + " checks passed");
    }
}
