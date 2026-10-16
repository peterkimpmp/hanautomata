using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.IO;
using System.Text;
using System.Threading;
using System.Web.Script.Serialization;
using System.Windows.Forms;

namespace Hanautomata
{
    internal sealed class ProofreadingWindow : Form
    {
        readonly Label status=new Label { Height=62, Dock=DockStyle.Top };
        readonly ComboBox choices=new ComboBox { DropDownStyle=ComboBoxStyle.DropDownList, Dock=DockStyle.Top };
        readonly Button apply=new Button { Text="선택한 제안 적용", AutoSize=true, Enabled=false };
        readonly List<Tuple<int,int>> rows=new List<Tuple<int,int>>();
        readonly JavaScriptSerializer json=new JavaScriptSerializer { MaxJsonLength=262144, RecursionLimit=32 };
        readonly Func<bool> unchanged;
        readonly Func<string,bool> replace;
        readonly ProofRequest request;
        readonly bool copyOnly;
        readonly List<string> protectedWords=new List<string>();
        Process worker;
        ProofReport report;
        int generation;
        bool checking;
        internal ProofreadingWindow(ProofRequest request,Func<bool> unchanged,Func<string,bool> replace,bool copyOnly=false)
        {
            this.request=request; this.unchanged=unchanged; this.replace=replace;
            this.copyOnly=copyOnly; if (copyOnly) apply.Text="선택한 제안 복사";
            Text="한국어 표기 제안 · 실험 기능"; Font=new Font("Malgun Gothic",10); ClientSize=new Size(620,330);
            StartPosition=FormStartPosition.CenterScreen; TopMost=true; MaximizeBox=false; MinimizeBox=false;
            var body=new Panel { Dock=DockStyle.Fill, Padding=new Padding(20) }; Controls.Add(body);
            var buttons=new FlowLayoutPanel { Dock=DockStyle.Bottom, Height=45 };
            var recheck=new Button { Text="다시 검사", AutoSize=true }; recheck.Click+=delegate { Check(); };
            var cancel=new Button { Text="검사 취소", AutoSize=true }; cancel.Click+=delegate { Cancel(); };
            var protect=new Button { Text="이 단어 보호", AutoSize=true }; protect.Click+=delegate {
                if (report==null || choices.SelectedIndex<0 || protectedWords.Count>=256) return;
                string word=report.diagnostics[rows[choices.SelectedIndex].Item1].original;
                if (!protectedWords.Contains(word)) protectedWords.Add(word); Check();
            };
            apply.Click+=delegate { Apply(); }; buttons.Controls.AddRange(new Control[] {recheck,cancel,protect,apply});
            body.Controls.Add(buttons); body.Controls.Add(choices); body.Controls.Add(status);
            body.Controls.Add(new Label { Dock=DockStyle.Top, Height=84, Text=request.text.Length>240 ? request.text.Substring(0,240)+"…" : request.text });
            body.Controls.Add(new Label { Dock=DockStyle.Top, Height=54, ForeColor=Color.DimGray,
                Text="선택한 글을 이 기기에서 검사합니다.\n사전에 없는 단어는 확인 후보이며 오류로 확정하지 않습니다." });
            Shown+=delegate { Check(); }; FormClosed+=delegate { Stop(); };
        }
        protected override bool ShowWithoutActivation { get { return true; } }
        protected override CreateParams CreateParams { get { var p=base.CreateParams; p.ExStyle|=0x08000000; return p; } }
        protected override void WndProc(ref Message m)
        { if (m.Msg==0x21) { m.Result=new IntPtr(3); return; } base.WndProc(ref m); }
        void Clear(string message) { report=null; rows.Clear(); choices.Items.Clear(); apply.Enabled=false; status.Text=message; checking=false; }
        void Check()
        {
            Cancel();
            if (!unchanged()) { Clear("원문·선택 영역·입력 칸이 바뀌었습니다. 창을 닫고 다시 선택해 주세요."); return; }
            Clear("검사 중… 입력은 계속하실 수 있습니다.");
            request.request_id=Guid.NewGuid().ToString("N"); request.protected_words=protectedWords.ToArray(); checking=true;
            try {
                if (worker==null || worker.HasExited) StartWorker();
                Send(new { schema_version=1, command="check", request=request });
            } catch { Stop(); Clear("검사기가 없습니다. 맞춤법 패키지가 포함된 빌드를 사용해 주세요."); }
        }
        void StartWorker()
        {
            Stop(); int lease=generation;
            string directory=Path.Combine(AppDomain.CurrentDomain.BaseDirectory,"proofreading");
            var start=new ProcessStartInfo(Path.Combine(directory,"hanautomata-proofread.exe"),
                "--serve --rules --dictionary \""+Path.Combine(directory,"data")+"\"") {
                UseShellExecute=false, CreateNoWindow=true, RedirectStandardInput=true, RedirectStandardOutput=true,
                RedirectStandardError=true, StandardOutputEncoding=new UTF8Encoding(false,true), StandardErrorEncoding=new UTF8Encoding(false,true) };
            worker=Process.Start(start); var process=worker;
            ThreadPool.QueueUserWorkItem(delegate {
                try {
                    var decoder=new JavaScriptSerializer { MaxJsonLength=262144, RecursionLimit=32 };
                    string line;
                    while ((line=ReadFrame(process.StandardOutput,262144))!=null) {
                        var reply=decoder.Deserialize<ProofReply>(line);
                        if (IsDisposed) break;
                        BeginInvoke((Action)delegate { if (generation==lease) Received(reply); });
                    }
                } catch { }
                if (!IsDisposed) try { BeginInvoke((Action)delegate { if (generation==lease) { Stop(); Clear("검사기가 종료되었습니다. 다시 검사해 주세요."); } }); } catch { }
            });
        }
        internal static string ReadFrame(TextReader reader,int maximum)
        {
            var line=new StringBuilder(); int c;
            while ((c=reader.Read())!=-1) {
                if (c=='\n') return line.ToString();
                if (line.Length>=maximum) throw new InvalidDataException();
                line.Append((char)c);
            }
            if (line.Length!=0) throw new InvalidDataException(); return null;
        }
        void Send(object value)
        {
            var process=worker; if (process==null) return;
            byte[] bytes=Encoding.UTF8.GetBytes(json.Serialize(value)+"\n"); if (bytes.Length>65536) throw new InvalidDataException();
            lock (process) { process.StandardInput.BaseStream.Write(bytes,0,bytes.Length); process.StandardInput.BaseStream.Flush(); }
        }
        void Received(ProofReply reply)
        {
            if (reply==null || reply.@event=="ready" || reply.request_id!=request.request_id || !checking) return;
            checking=false;
            if (reply.@event!="result" || !ProofreadingContract.Matches(request,reply.report)) { Clear("검사를 마치지 못했습니다. 다시 검사해 주세요."); return; }
            report=reply.report;
            for (int d=0;d<report.diagnostics.Length;d++) {
                var item=report.diagnostics[d];
                for (int c=0;c<item.suggestions.Length;c++) {
                    rows.Add(Tuple.Create(d,c)); choices.Items.Add(item.original+" → "+item.suggestions[c]+(item.rule_id==null ? " · 사전 후보" : " · 규칙 제안"));
                }
            }
            if (rows.Count>0) choices.SelectedIndex=0; apply.Enabled=rows.Count>0;
            status.Text=report.diagnostics.Length==0 ? "제안을 찾지 못했습니다. 문장 전체의 정확성을 보증하지 않습니다." : "확인 후보 "+report.diagnostics.Length+"개 · 제안을 직접 선택해 주세요.";
            if (report.limited) status.Text+=" 검사 한도에 도달했습니다. 짧게 선택해 주세요.";
        }
        void Apply()
        {
            if (report==null || choices.SelectedIndex<0 || !unchanged()) { Clear("원문 또는 입력 칸이 바뀌었습니다. 다시 선택해 주세요."); return; }
            var row=rows[choices.SelectedIndex]; string value=ProofreadingContract.Preview(request,report,row.Item1,row.Item2);
            if (value==null || !replace(value)) { Clear("원문을 바꾸지 못했습니다. 다시 선택해 주세요."); return; }
            if (!IsDisposed) Clear(copyOnly ? "제안을 복사했습니다. 원래 앱에서 붙여넣어 주세요." : "제안 한 개를 적용했습니다. 원래 앱의 실행 취소로 되돌릴 수 있습니다.");
        }
        void Cancel() { if (checking) try { Send(new { schema_version=1, command="cancel", request_id=request.request_id }); } catch { Stop(); } Clear("검사를 취소했습니다."); }
        void Stop()
        {
            generation++; var process=worker; worker=null; if (process==null) return;
            try { process.StandardInput.Close(); } catch { }
            ThreadPool.QueueUserWorkItem(delegate { try { if (!process.WaitForExit(4000)) process.Kill(); process.WaitForExit(); } catch { } finally { process.Dispose(); } });
        }
    }
    internal static class ProofreadingProgram
    {
        [STAThread] public static int Main()
        {
            try {
                // TSF starts this WinExe with redirected pipes and no console.
                // Console encoding setters can throw when no console is attached.
                using (var input=new StreamReader(Console.OpenStandardInput(),new UTF8Encoding(false,true),false))
                using (var output=new StreamWriter(Console.OpenStandardOutput(),new UTF8Encoding(false)) { AutoFlush=true }) {
                var json=new JavaScriptSerializer { MaxJsonLength=65536, RecursionLimit=16 };
                var request=json.Deserialize<ProofRequest>(ProofreadingWindow.ReadFrame(input,65536));
                if (request==null || string.IsNullOrEmpty(request.text) || Encoding.UTF8.GetByteCount(request.text)>16384) return 2;
                string id=request.request_id, document=request.document_id, revision=request.revision, original=request.text;
                Application.EnableVisualStyles(); ProofreadingWindow window=null;
                window=new ProofreadingWindow(request,delegate { return true; },delegate(string value) {
                    output.WriteLine(json.Serialize(new { schema_version=1, request_id=id, document_id=document, revision=revision, original=original, text=value }));
                    window.Close(); return true;
                });
                Application.Run(window); return 0;
                }
            } catch { return 2; }
        }
    }
}
