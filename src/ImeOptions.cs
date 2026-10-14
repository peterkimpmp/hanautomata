using System;
using System.Drawing;
using System.IO;
using System.Linq;
using System.Collections.Generic;
using System.Windows.Forms;

namespace Hanautomata
{
    internal static class ImeOptionsProgram
    {
        [STAThread]
        static int Main(string[] args)
        {
            if (args.Length == 0) return Run(ImeSettingsStore.DefaultPath);
            if (args.Length == 2 && args[0] == "--settings-file") return Run(args[1]);
            return 2;
        }
        internal static int Run(string path)
        {
            try { path = Path.GetFullPath(path); }
            catch (ArgumentException) { return 2; }
            catch (NotSupportedException) { return 2; }
            catch (PathTooLongException) { return 2; }
            Application.EnableVisualStyles(); Application.SetCompatibleTextRenderingDefault(false);
            using (var form = new ImeOptionsWindow(path)) Application.Run(form);
            return 0;
        }
    }

    internal sealed class ImeOptionsWindow : Form
    {
        sealed class Choice
        {
            internal string Value, Label;
            internal Choice(string value, string label) { Value = value; Label = label; }
            public override string ToString() { return Label; }
        }
        readonly string path;
        ImeSettingsSnapshot snapshot;
        readonly Dictionary<string, Control> bindings = new Dictionary<string, Control>();
        readonly TabControl tabs = new TabControl { Dock = DockStyle.Fill };
        readonly TextBox excluded = new TextBox { Multiline = true, ScrollBars = ScrollBars.Vertical, Dock = DockStyle.Fill, AcceptsReturn = true, Height = 150, AccessibleName = "제외할 앱 실행 파일 이름" };
        readonly TextBox status = new TextBox { Multiline = true, ReadOnly = true, WordWrap = true, ScrollBars = ScrollBars.Vertical, Dock = DockStyle.Fill, Height = 96, AccessibleName = "설정 저장 상태" };
        readonly Button save = new Button { Text = "저장", AutoSize = true, Enabled = false };
        readonly Button reset = new Button { Text = "기본값", AutoSize = true };
        readonly Button recover = new Button { Text = "파일에서 복원…", AutoSize = true };
        bool loading, dirty, restoring;

        internal ImeOptionsWindow(string settingsPath)
        {
            // Build the complete 96-DPI layout before WinForms scales it once.
            // Early layout otherwise consumes the scale factor before ClientSize and children exist.
            SuspendLayout();
            path = Path.GetFullPath(settingsPath);
            Text = "한오토마타 입력기 설정";
            Font = new Font("Malgun Gothic", 9F); StartPosition = FormStartPosition.CenterScreen;
            AutoScaleDimensions = new SizeF(96, 96); AutoScaleMode = AutoScaleMode.Dpi;
            ClientSize = new Size(660, 610); MinimumSize = new Size(560, 500);
            var layout = new TableLayoutPanel { Dock = DockStyle.Fill, Padding = new Padding(18), ColumnCount = 1, RowCount = 5 };
            layout.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            layout.SizeChanged += delegate { status.Height = status.Font.Height * 5 + 12; };
            layout.RowStyles.Add(new RowStyle(SizeType.AutoSize)); layout.RowStyles.Add(new RowStyle(SizeType.AutoSize));
            layout.RowStyles.Add(new RowStyle(SizeType.Percent, 100)); layout.RowStyles.Add(new RowStyle(SizeType.AutoSize)); layout.RowStyles.Add(new RowStyle(SizeType.AutoSize));
            layout.Controls.Add(new Label { Text = "한오토마타 · Windows 입력기", AutoSize = true, Font = new Font(Font.FontFamily, 16F, FontStyle.Bold), Margin = new Padding(0, 0, 0, 10) }, 0, 0);
            layout.Controls.Add(new Label { Text = "저장 후 입력기를 다른 입력기로 바꿨다가 한오토마타를 다시 선택해 주세요.\n실행 중인 앱에 적용되지 않으면 해당 앱을 다시 열어 주세요.", AutoSize = true, Dock = DockStyle.Fill, Margin = new Padding(0, 0, 0, 12) }, 0, 1);
            layout.Controls.Add(tabs, 0, 2); layout.Controls.Add(status, 0, 3);
            var input = Page("입력");
            Combo(input, "InputMode", "시작 입력 모드", new Choice("Auto", "자동 판별"), new Choice("Korean", "한글"), new Choice("English", "영어"));
            Combo(input, "Keyboard", "한글 자판", new Choice("Dubeolsik", "두벌식"), new Choice("Sebeolsik390", "세벌식 390"), new Choice("SebeolsikFinal", "세벌식 최종"));
            Combo(input, "SwitchKind", "전환 키 동작", new Choice("SingleShot", "다음 단어만 전환"), new Choice("Cycle", "자동 → 한글 → 영어 순환"), new Choice("Toggle", "한글 ↔ 영어 전환"));
            Combo(input, "Sensitivity", "한글 판별 감도", new Choice("conservative", "보수적"), new Choice("balanced", "균형"), new Choice("aggressive", "적극적"));
            Toggle(input, "SemicolonShortcut", "; 키로 다음 단어 전환");
            Toggle(input, "DeleteSyllable", "Backspace로 한글 음절 전체 삭제");
            Toggle(input, "MsClusters", "모음 없이 입력한 겹자음도 합치기 (ㄱ + ㅅ → ㄳ)");
            Toggle(input, "Strict", "통계적 한글 추정 대신 입력한 영문 보존");
            var keys = Page("단축키");
            Toggle(keys, "ShiftSpace", "Shift + Space로 전환"); Toggle(keys, "CapsLock", "짧은 Caps Lock으로 전환");
            Note(keys, "Caps Lock을 길게 누르거나 Shift와 함께 누르면 대문자 잠금을 사용합니다.");
            Toggle(keys, "HangulKey", "한/영 키로 전환"); Toggle(keys, "RightAlt", "오른쪽 Alt로 전환");
            Toggle(keys, "PauseKey", "F12로 입력기 일시 정지 / 복귀"); Toggle(keys, "CorrectionKeys", "F2 재변환 · Shift + F2 선택 영역 변환");
            var privacy = Page("학습·제외 앱");
            Toggle(privacy, "LearnChoices", "기억한 선택을 판별에 사용"); Toggle(privacy, "LearningPaused", "새로운 선택 학습 일시 정지");
            Note(privacy, "새 학습은 입력기를 다시 선택하거나 앱을 닫으면 사라질 수 있습니다.\n계속 보존하는 기능은 아직 준비 중입니다.");
            Note(privacy, "제외할 앱의 실행 파일 이름을 한 줄에 하나씩 적어 주세요.\n예: code.exe  (전체 경로는 입력하지 않습니다.)");
            privacy.Controls.Add(excluded); excluded.TextChanged += delegate { Changed(); };
            var buttons = new FlowLayoutPanel { AutoSize = true, Dock = DockStyle.Fill, FlowDirection = FlowDirection.RightToLeft };
            var close = new Button { Text = "닫기", AutoSize = true }; close.Click += delegate { Close(); };
            var reload = new Button { Text = "다시 읽기", AutoSize = true };
            reload.Click += delegate { if (MayDiscard()) LoadSettings(); };
            save.Click += delegate { SaveSettings(); }; reset.Click += delegate {
                if (snapshot.Fingerprint == null) return;
                if (MessageBox.Show(this, "제외 앱을 포함한 모든 옵션을 기본값으로 바꿀까요? 저장 시 직전 파일 1개를 .previous로 보관하며 다음 저장 때 교체됩니다.", Text, MessageBoxButtons.YesNo, MessageBoxIcon.Question) != DialogResult.Yes) return;
                Populate(new ImeSettings()); restoring = false; tabs.Enabled = true; Changed();
            };
            recover.Click += delegate { PreviewRecovery(); };
            buttons.Controls.AddRange(new Control[] { close, save, reload, recover, reset }); layout.Controls.Add(buttons, 0, 4);
            Controls.Add(layout); CancelButton = close;
            FormClosing += delegate(object sender, FormClosingEventArgs e) { if (!MayDiscard()) e.Cancel = true; };
            LoadSettings();
            ResumeLayout(true);
        }
        FlowLayoutPanel Page(string title)
        {
            var tab = new TabPage(title) { Padding = new Padding(12) };
            var panel = new FlowLayoutPanel { Dock = DockStyle.Fill, FlowDirection = FlowDirection.TopDown, WrapContents = false, AutoScroll = true };
            panel.SizeChanged += delegate { foreach (Control child in panel.Controls) child.Width = Math.Max(220, panel.ClientSize.Width - 24); };
            tab.Controls.Add(panel); tabs.TabPages.Add(tab); return panel;
        }
        void Note(FlowLayoutPanel page, string text)
        {
            var note = new Label { Text = text, AutoSize = false, Margin = new Padding(3, 6, 3, 6) };
            EventHandler fit = delegate {
                int needed = TextRenderer.MeasureText(note.Text, note.Font, new Size(Math.Max(1, note.ClientSize.Width), int.MaxValue),
                    TextFormatFlags.WordBreak | TextFormatFlags.TextBoxControl).Height;
                if (note.Height != needed) note.Height = needed;
            };
            note.SizeChanged += fit; note.FontChanged += fit;
            page.Controls.Add(note); fit(note, EventArgs.Empty);
        }
        void Combo(FlowLayoutPanel page, string property, string label, params Choice[] choices)
        {
            var row = new TableLayoutPanel { ColumnCount = 2, Height = 35, Margin = new Padding(3, 3, 3, 6) };
            row.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 40)); row.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 60));
            row.Controls.Add(new Label { Text = label, AutoSize = true, Anchor = AnchorStyles.Left }, 0, 0);
            var box = new ComboBox { DropDownStyle = ComboBoxStyle.DropDownList, Dock = DockStyle.Fill, AccessibleName = label };
            box.Items.AddRange(choices); box.SelectedIndexChanged += delegate { Changed(); };
            bindings.Add(property, box); row.Controls.Add(box, 1, 0); page.Controls.Add(row);
        }
        void Toggle(FlowLayoutPanel page, string property, string label)
        {
            var box = new CheckBox { Text = label, AutoSize = false, Height = 34, Margin = new Padding(3), AccessibleName = label };
            bindings.Add(property, box); box.CheckedChanged += delegate { Changed(); }; page.Controls.Add(box);
        }
        void Populate(ImeSettings value)
        {
            loading = true;
            foreach (var entry in bindings)
            {
                object v = typeof(ImeSettings).GetProperty(entry.Key).GetValue(value, null);
                var box = entry.Value as ComboBox;
                if (box != null) box.SelectedItem = box.Items.Cast<Choice>().First(c => c.Value == (string)v);
                else ((CheckBox)entry.Value).Checked = (bool)v;
            }
            excluded.Text = string.Join(Environment.NewLine, value.ExcludedApps); loading = false;
        }
        void SetStatus(string text) { status.Text = text.Replace("\r\n", "\n").Replace("\n", Environment.NewLine); }
        void Changed() { if (!loading) { dirty = true; save.Enabled = snapshot != null && snapshot.Fingerprint != null; SetStatus(restoring ? "복원 미리보기를 변경했습니다. 아직 저장하지 않았습니다." : "변경 사항을 저장하지 않았습니다."); } }
        void LoadSettings()
        {
            snapshot = ImeSettingsStore.Load(path); Populate(snapshot.Value ?? new ImeSettings());
            dirty = false; restoring = false; save.Enabled = false; tabs.Enabled = snapshot.Value != null;
            reset.Enabled = recover.Enabled = snapshot.Fingerprint != null;
            SetStatus(snapshot.Error ?? "설정을 읽었습니다. 저장한 설정은 입력기를 다시 선택할 때 적용됩니다.");
            if (snapshot.Error != null) SetStatus(status.Text + (snapshot.Fingerprint == null ? "\n파일 크기와 접근 권한을 확인한 뒤 다시 읽어 주세요." : "\n‘파일에서 복원’으로 기존 옵션을 선택하거나 ‘기본값’을 사용해 주세요."));
        }
        void PreviewRecovery()
        {
            using (var dialog = new OpenFileDialog { Title = "복원할 한오토마타 설정 선택", CheckFileExists = true, RestoreDirectory = true,
                InitialDirectory = Path.GetDirectoryName(path),
                Filter = "보관된 설정 (*.previous;*.tmp;*.json)|*.previous;*.tmp;*.json|모든 파일 (*.*)|*.*" })
            {
                if (dialog.ShowDialog(this) != DialogResult.OK) return;
                try
                {
                    var current = ImeSettingsStore.Load(path);
                    if (current.Fingerprint == null) { SetStatus("현재 설정 파일을 확인할 수 없어 복원을 준비하지 못했습니다. " + current.Error); return; }
                    var value = ImeSettingsStore.ReadRecovery(dialog.FileName);
                    if (!MayDiscard()) return;
                    snapshot = current; Populate(value); tabs.Enabled = true; restoring = true; Changed();
                    SetStatus("복원 미리보기: " + Path.GetFileName(dialog.FileName) + "\n옵션을 확인하고 저장을 눌러 주세요. 이번 복원 저장은 보관 파일을 유지하고 현재 파일을 별도로 백업합니다.");
                }
                catch (Exception e) { SetStatus("복원 파일을 읽지 못했습니다. " + e.Message); }
            }
        }
        void SaveSettings()
        {
            try
            {
                var value = new ImeSettings();
                foreach (var entry in bindings)
                {
                    var box = entry.Value as ComboBox;
                    typeof(ImeSettings).GetProperty(entry.Key).SetValue(value, box == null ? (object)((CheckBox)entry.Value).Checked : ((Choice)box.SelectedItem).Value, null);
                }
                value.ExcludedApps = excluded.Text.Split(new[] { '\r', '\n' }, StringSplitOptions.RemoveEmptyEntries).Select(x => x.Trim()).Where(x => x.Length > 0).ToArray();
                try { snapshot = ImeSettingsStore.Save(path, value, snapshot.Fingerprint, restoring); }
                catch (ImeSettingsConflictException)
                {
                    if (!restoring) throw;
                    // Keep the preview even if another window rotated .previous. Never silently advance its fingerprint.
                    if (MessageBox.Show(this, "설정 파일의 상태가 바뀌었습니다. 현재 파일이 있으면 별도로 백업한 뒤 복원 미리보기로 바꿀까요?", Text, MessageBoxButtons.YesNo, MessageBoxIcon.Question) != DialogResult.Yes)
                    { SetStatus("복원 미리보기를 유지했습니다. 다시 저장을 누르면 최신 설정 교체 여부를 확인합니다."); return; }
                    var latest = ImeSettingsStore.Load(path);
                    snapshot = ImeSettingsStore.Save(path, value, latest.Fingerprint, true);
                }
                dirty = false; restoring = false; save.Enabled = snapshot.Warning != null;
                SetStatus(snapshot.Warning ?? "저장했습니다. 입력기를 다시 선택해 주세요. 반영되지 않으면 앱을 다시 열어 주세요.");
                if (snapshot.BackupPath != null) SetStatus(status.Text + "\n이전 파일: " + snapshot.BackupPath);
            }
            catch (ImeSettingsConflictException e) { SetStatus(restoring ? "설정 파일의 상태가 다시 바뀌었습니다. 복원 미리보기를 유지했으니 다시 저장을 눌러 주세요." : "저장하지 못했습니다. " + e.Message); }
            catch (Exception e) { SetStatus("저장하지 못했습니다. " + e.Message); }
        }
        bool MayDiscard() { return !dirty || MessageBox.Show(this, "저장하지 않은 변경 사항을 버릴까요?", Text, MessageBoxButtons.YesNo, MessageBoxIcon.Question) == DialogResult.Yes; }
    }
}
