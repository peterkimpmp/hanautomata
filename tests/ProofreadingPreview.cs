using System;
using System.Drawing;
using System.Windows.Forms;

namespace Hanautomata
{
    // Isolated real WinForms editor: no hooks, IME registration, clipboard or other applications.
    internal static class ProofreadingPreview
    {
        [STAThread] public static void Main()
        {
            Application.EnableVisualStyles();
            var host=new Form { Text="Hanautomata · 맞춤법 검증용 편집기", ClientSize=new Size(760,360) };
            var editor=new TextBox { Multiline=true, AcceptsReturn=true, Dock=DockStyle.Fill,
                Font=new Font("Malgun Gothic",16), Text="안녕하세요. 오늘은 일이 잘 됬다. 내일은 할수있다. 이제 되요.", HideSelection=false };
            var commands=new FlowLayoutPanel { Dock=DockStyle.Bottom, Height=48 };
            var inspect=new Button { Text="선택한 글 검사", AutoSize=true };
            var undo=new Button { Text="실행 취소", AutoSize=true };
            var select=new Button { Text="모두 선택", AutoSize=true };
            commands.Controls.AddRange(new Control[] {inspect,undo,select}); host.Controls.Add(editor); host.Controls.Add(commands);
            int revision=0;
            editor.TextChanged+=delegate { revision++; };
            select.Click+=delegate { editor.Focus(); editor.SelectAll(); };
            undo.Click+=delegate { editor.Focus(); editor.Undo(); };
            inspect.Click+=delegate {
                editor.Focus();
                int start=editor.SelectionStart, length=editor.SelectionLength, version=revision;
                string original=editor.SelectedText;
                if (length==0) return;
                Func<bool> same=delegate { return host.ContainsFocus && editor.Focused && revision==version
                    && editor.SelectionStart==start && editor.SelectionLength==length && editor.SelectedText==original; };
                var request=new ProofRequest { request_id=Guid.NewGuid().ToString("N"), document_id="preview",
                    revision=version.ToString(), text=original, protected_words=new string[0] };
                var review=new ProofreadingWindow(request,same,delegate(string replacement) {
                    if (!same()) return false;
                    // EM_REPLACESEL with undo enabled, on this owned editor only.
                    editor.SelectedText=replacement; return true;
                });
                review.Show();
            };
            host.Shown+=delegate { editor.Focus(); editor.SelectAll(); };
            Application.Run(host);
        }
    }
}
