using System;
using System.Collections.Generic;
using System.Drawing;
using System.IO;
using System.Windows.Forms;

namespace Hanautomata
{
    internal sealed class PersonalAssistWindow : Form
    {
        static PersonalAssistWindow current;
        readonly PersonalAssistClient service;
        readonly TextBox search=new TextBox { Dock=DockStyle.Fill, MaxLength=512, AccessibleName="표현 찾기" };
        readonly TextBox expression=new TextBox { Dock=DockStyle.Fill, MaxLength=512, AccessibleName="새 표현" };
        readonly CheckBox enabled=new CheckBox { Text="추천 사용 · 복사 선택을 순위에 반영", AutoSize=true };
        readonly Label status=new Label { AutoSize=true, Dock=DockStyle.Fill, AccessibleName="추천 상태" };
        readonly ListBox items=new ListBox { Dock=DockStyle.Fill, HorizontalScrollbar=true, AccessibleName="내 표현 목록" };
        readonly Button copy=Button("복사"), remove=Button("삭제"), undo=Button("최근 순위 기억 되돌리기"), clear=Button("전체 삭제"), retry=Button("다시 불러오기"), add=Button("등록");
        readonly Timer debounce=new Timer { Interval=150 };
        bool busy, updating, refreshPending, savedEnabled; int revision; string undoId="", notice="";
        static Button Button(string text) { return new Button { Text=text, AutoSize=true, Padding=new Padding(5), AccessibleName=text }; }

        internal static void ShowPanel()
        {
            if(current==null || current.IsDisposed) current=new PersonalAssistWindow();
            current.Show(); current.Activate();
        }
        internal PersonalAssistWindow()
        {
            Text="내 표현 · 이 기기에서 처리"; Font=new Font("Malgun Gothic",10);
            AutoScaleMode=AutoScaleMode.Dpi; ClientSize=new Size(760,580); MinimumSize=new Size(680,560);
            StartPosition=FormStartPosition.CenterScreen;
            service=new PersonalAssistClient(Path.Combine(AppDomain.CurrentDomain.BaseDirectory,"hanautomata-assist.exe"),
                Path.Combine(Settings.DirectoryPath,"personal-expressions.json"));
            var layout=new TableLayoutPanel { Dock=DockStyle.Fill, Padding=new Padding(20), ColumnCount=1, RowCount=11 };
            for(int i=0;i<11;i++) layout.RowStyles.Add(new RowStyle(i==5 ? SizeType.Percent : SizeType.AutoSize, i==5 ? 100 : 0));
            layout.Controls.Add(new Label { Text="내 단어와 표현을 빠르게 찾으세요", AutoSize=true, Font=new Font(Font.FontFamily,16,FontStyle.Bold) },0,0);
            layout.Controls.Add(new Label { Text="직접 등록한 표현만 이 기기에 평문으로 저장합니다. 입력 내용은 자동 수집하지 않습니다.", AutoSize=true, MaximumSize=new Size(690,0), Margin=new Padding(0,10,0,10) },0,1);
            layout.Controls.Add(enabled,0,2);
            layout.Controls.Add(new Label { Text="표현 찾기 · 두 글자 이상 / 비우면 전체 목록", AutoSize=true, Margin=new Padding(0,14,0,4) },0,3);
            layout.Controls.Add(search,0,4); layout.Controls.Add(items,0,5);
            var actions=new FlowLayoutPanel { AutoSize=true, Dock=DockStyle.Fill, WrapContents=true };
            actions.Controls.AddRange(new Control[] { copy, remove, undo, clear, retry }); layout.Controls.Add(actions,0,6);
            layout.Controls.Add(status,0,7);
            layout.Controls.Add(new Label { Text="새 표현 · 한 줄 2~256자, 최대 512개", AutoSize=true, Margin=new Padding(0,14,0,4) },0,8);
            layout.Controls.Add(expression,0,9); layout.Controls.Add(add,0,10); Controls.Add(layout);
            search.TextChanged+=delegate { revision++; items.Items.Clear(); UpdateControls(); debounce.Stop(); debounce.Start(); };
            debounce.Tick+=delegate { debounce.Stop(); RefreshItems(); };
            enabled.CheckedChanged+=delegate { if(!updating) Send("set_enabled", Fields("enabled",enabled.Checked),false,"추천 설정을 저장했습니다."); };
            add.Click+=delegate { Send("add",Fields("text",expression.Text),false,"표현을 저장했습니다."); };
            copy.Click+=delegate { Copy(); };
            remove.Click+=delegate { var item=items.SelectedItem as AssistItem; if(item!=null) Send("delete",Fields("entry_id",item.id),false,"표현을 삭제했습니다."); };
            undo.Click+=delegate { Send("undo",Fields("undo_id",undoId),false,"최근 순위 기억을 복원했습니다. 문서와 클립보드는 바꾸지 않았습니다."); };
            clear.Click+=delegate {
                if(MessageBox.Show(this,"직접 등록한 모든 표현과 순위 기억을 삭제할까요?","내 표현 전체 삭제",MessageBoxButtons.OKCancel,MessageBoxIcon.Question)==DialogResult.OK)
                    Send("clear",Fields(),false,"모든 표현을 삭제했습니다.");
            };
            retry.Click+=delegate { RefreshItems(); };
            items.SelectedIndexChanged+=delegate { UpdateControls(); };
            Shown+=delegate { RefreshItems(); search.Focus(); };
            FormClosed+=delegate { debounce.Dispose(); service.Dispose(); };
            UpdateControls();
        }
        static Dictionary<string,object> Fields() { return new Dictionary<string,object>(); }
        static Dictionary<string,object> Fields(string key,object value) { var f=Fields(); f[key]=value; return f; }
        void UpdateControls()
        {
            copy.Enabled=remove.Enabled=!busy && items.SelectedItem!=null;
            undo.Enabled=!busy && undoId.Length>0; add.Enabled=clear.Enabled=retry.Enabled=enabled.Enabled=!busy;
            expression.Enabled=!busy;
        }
        void RefreshItems()
        {
            if(busy) { refreshPending=true; return; }
            bool all=search.Text.Trim().Length==0;
            Send(all ? "list" : "query",all ? Fields() : Fields("text",search.Text),true,"");
        }
        void Send(string command,Dictionary<string,object> fields,bool read,string message)
        {
            if(busy) return;
            busy=true; int stamp=revision; UpdateControls(); status.Text="불러오는 중…";
            service.Send(command,fields,delegate(AssistReply reply) {
                if(IsDisposed || !IsHandleCreated) return;
                try { BeginInvoke((Action)delegate {
                    if(IsDisposed) return;
                    busy=false;
                    if(!reply.ok) {
                        items.Items.Clear(); status.Text=(command=="copied" ? "복사는 완료됐지만 순위를 저장하지 못했습니다. " : "")+PersonalAssistClient.Message(reply.error); undoId="";
                        updating=true; enabled.Checked=savedEnabled; updating=false;
                    } else {
                        savedEnabled=reply.enabled; updating=true; enabled.Checked=reply.enabled; updating=false;
                        if(!read) { undoId=reply.undo_id ?? ""; notice=message; if(command=="add") expression.Clear(); }
                        if(stamp==revision && (read || search.Text.Trim().Length==0)) {
                            items.Items.Clear(); foreach(var item in reply.items) items.Items.Add(item);
                            if(items.Items.Count>0) items.SelectedIndex=0;
                            status.Text=read ? (search.Text.Trim().Length==0 ? "저장한 표현 "+items.Items.Count+"개 · 선택 후 복사하거나 삭제하세요." :
                                !reply.enabled ? "추천 사용을 켜면 등록한 표현을 찾을 수 있습니다." : items.Items.Count==0 ? "일치하는 표현이 없습니다. 두 글자 이상으로 찾아보세요." : "내 표현 추천 "+items.Items.Count+"개 · 복사해서 사용하세요.") : message;
                            if(read && notice.Length>0) { status.Text=notice+" "+status.Text; notice=""; }
                        } else refreshPending=true;
                    }
                    UpdateControls();
                    if(refreshPending || stamp!=revision) { refreshPending=false; RefreshItems(); }
                }); } catch(InvalidOperationException) {}
            });
        }
        void Copy()
        {
            var item=items.SelectedItem as AssistItem; if(item==null || busy) return;
            try { Clipboard.SetText(item.text); }
            catch { status.Text="클립보드에 복사하지 못했습니다. 다시 눌러 주세요. 순위는 바꾸지 않았습니다."; return; }
            if(enabled.Checked) Send("copied",Fields("entry_id",item.id),false,"복사했습니다. 선택을 순위에 반영했습니다.");
            else status.Text="복사했습니다. 추천이 꺼져 있어 순위는 바꾸지 않았습니다.";
        }
    }
}
