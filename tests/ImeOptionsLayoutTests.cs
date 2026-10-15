using System;
using System.Drawing;
using System.IO;
using System.Linq;
using System.Collections.Generic;
using System.Windows.Forms;
using Hanautomata;

// In-process layout regression only: never shows a window or edits live settings.
internal static class ImeOptionsLayoutTests
{
    static int failures;
    static void Check(string name, bool ok, string detail)
    {
        Console.WriteLine((ok ? "PASS " : "FAIL ") + name + " " + detail);
        if (!ok) failures++;
    }
    static IEnumerable<Control> Descendants(Control root)
    {
        foreach (Control child in root.Controls)
        {
            yield return child;
            foreach (Control nested in Descendants(child)) yield return nested;
        }
    }
    [STAThread]
    static int Main()
    {
        Application.EnableVisualStyles(); Application.SetCompatibleTextRenderingDefault(false);
        string denied = Path.Combine(Path.GetTempPath(), "hanautomata-denied-" + Guid.NewGuid().ToString("N"), "settings.json");
        Check("administrator preparation refuses before writing", ImeOptionsProgram.PrepareUser(denied, true) == 3 && !Directory.Exists(Path.GetDirectoryName(denied)), "no administrator settings created");
        using (var form = new ImeOptionsWindow(Path.Combine(Path.GetTempPath(), "hanautomata-layout-" + Guid.NewGuid().ToString("N"), "settings.json")))
        {
            // Materialize native tab metrics while the form remains hidden. Uncreated tabs report a placeholder viewport.
            foreach (Control control in Descendants(form).ToArray()) { IntPtr handle = control.Handle; }
            form.PerformLayout();
            var scale = form.CurrentAutoScaleDimensions;
            Check("initial client dimensions", form.ClientSize.Width >= 660 * scale.Width / 96 - 1 && form.ClientSize.Height >= 610 * scale.Height / 96 - 1,
                "client=" + form.ClientSize + " dpi=" + scale);
            var tabs = Descendants(form).OfType<TabControl>().Single();
            foreach (TabPage page in tabs.TabPages)
            {
                tabs.SelectedTab = page;
                foreach (Control control in Descendants(form).ToArray()) control.PerformLayout();
                if (page.Text == "기억한 선택")
                {
                    foreach (var button in Descendants(page).OfType<Button>())
                    {
                        var bottom = page.PointToClient(button.PointToScreen(new Point(button.Width, button.Height)));
                        Check("learning actions fit tab viewport", bottom.Y <= page.ClientSize.Height && bottom.X <= page.ClientSize.Width,
                            button.Text + " bottom=" + bottom + " viewport=" + page.ClientSize);
                    }
                }
                var panel = page.Controls.OfType<FlowLayoutPanel>().SingleOrDefault();
                if (page.Text == "입력" || page.Text == "단축키")
                {
                    int bottom = panel.Controls.Cast<Control>().Max(control => control.Bottom + control.Margin.Bottom) - panel.AutoScrollPosition.Y;
                    Check("default tab exposes all short options", bottom <= panel.ClientSize.Height,
                        page.Text + " content=" + bottom + " viewport=" + panel.ClientSize.Height);
                }
            }
            // Isolate child text measurement from the flow host: its scrollbar can resize
            // the viewport and restore every child's full width during a narrow-width check.
            var panels = tabs.TabPages.Cast<TabPage>().SelectMany(page => page.Controls.OfType<FlowLayoutPanel>()).ToArray();
            foreach (var panel in panels) panel.SuspendLayout();
            foreach (int width in new[] { 220, 500 })
            {
                foreach (CheckBox box in Descendants(tabs).OfType<CheckBox>())
                {
                    box.Width = width; box.PerformLayout();
                    int textWidth = Math.Max(1, box.ClientSize.Width - SystemInformation.MenuCheckSize.Width - box.Padding.Horizontal);
                    int needed = TextRenderer.MeasureText(box.Text, box.Font, new Size(textWidth, int.MaxValue),
                        TextFormatFlags.WordBreak | TextFormatFlags.TextBoxControl).Height + box.Padding.Vertical;
                    Check("checkbox text fits", box.Width == width && box.Height >= needed,
                        box.AccessibleName + " width=" + width + " measuredWidth=" + box.Width + " actual=" + box.Height + " needed=" + needed);
                }
                foreach (TableLayoutPanel row in Descendants(tabs).OfType<TableLayoutPanel>().Where(row => row.Controls.OfType<ComboBox>().Any()))
                {
                    row.Width = width; row.PerformLayout();
                    var label = row.Controls.OfType<Label>().Single();
                    int needed = label.GetPreferredSize(new Size(label.Width, 0)).Height;
                    Check("combo label fits", row.Width == width && label.Height >= needed && label.Bottom <= row.ClientSize.Height,
                        label.Text + " width=" + width + " measuredWidth=" + row.Width + " actual=" + label.Height + " needed=" + needed);
                }
            }
            var notes = Descendants(tabs).OfType<Label>().Where(label => label.Text.Contains("\n")).ToArray();
            foreach (int width in new[] { 220, 500 })
            {
                foreach (Label note in notes)
                {
                    // Exercise wrapped text at narrow and normal widths independently of monitor DPI.
                    note.Width = width;
                    note.PerformLayout();
                    int needed = TextRenderer.MeasureText(note.Text, note.Font, new Size(width, int.MaxValue), TextFormatFlags.WordBreak | TextFormatFlags.TextBoxControl).Height;
                    Check("wrapped note height", note.Width == width && note.Height >= needed, "width=" + width + " measuredWidth=" + note.Width + " actual=" + note.Height + " needed=" + needed);
                }
            }
            foreach (var panel in panels) panel.ResumeLayout();
        }
        return failures == 0 ? 0 : 1;
    }
}
