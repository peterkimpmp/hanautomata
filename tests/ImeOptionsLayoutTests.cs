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
            form.PerformLayout();
            var scale = form.CurrentAutoScaleDimensions;
            Check("initial client dimensions", form.ClientSize.Width >= 660 * scale.Width / 96 - 1 && form.ClientSize.Height >= 610 * scale.Height / 96 - 1,
                "client=" + form.ClientSize + " dpi=" + scale);
            var tabs = Descendants(form).OfType<TabControl>().Single();
            foreach (TabPage page in tabs.TabPages)
            {
                tabs.SelectedTab = page;
                foreach (Control control in Descendants(form).ToArray()) control.PerformLayout();
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
                    Check("wrapped note height", note.Height >= needed, "width=" + width + " actual=" + note.Height + " needed=" + needed);
                }
            }
        }
        return failures == 0 ? 0 : 1;
    }
}
