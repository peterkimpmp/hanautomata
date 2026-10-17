using System;
using System.Collections.Generic;
using System.Drawing;
using System.IO;
using System.Linq;
using System.Windows.Forms;
using Hanautomata;

namespace Hanautomata
{
    // The window's default constructor names the live settings directory; this fixture never calls it.
    internal static class Settings { internal static readonly string DirectoryPath = Path.Combine(Path.GetTempPath(), "hanautomata-assist-layout-unused"); }
}

// In-process layout regression only: never shows the window, starts the helper or touches a live store.
internal static class PersonalAssistLayoutTests
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
    static void Layout(Form form)
    {
        form.PerformLayout();
        foreach (Control control in Descendants(form).ToArray()) control.PerformLayout();
    }
    static void CheckFits(Form form, string size)
    {
        var layout = Descendants(form).OfType<TableLayoutPanel>().Single();
        Control previous = null;
        foreach (Control child in layout.Controls.Cast<Control>().OrderBy(control => layout.GetRow(control)))
        {
            string name = child.AccessibleName ?? child.Text;
            Check("row fits the window width (" + size + ")", child.Right <= layout.ClientSize.Width - layout.Padding.Right + 1,
                "[" + name + "] right=" + child.Right + " viewport=" + layout.ClientSize.Width);
            Check("row fits the window height (" + size + ")", child.Bottom <= layout.ClientSize.Height,
                "[" + name + "] bottom=" + child.Bottom + " viewport=" + layout.ClientSize.Height);
            if (previous != null)
                Check("row starts below the previous row (" + size + ")", child.Top >= previous.Bottom,
                    "[" + name + "] top=" + child.Top + " previous bottom=" + previous.Bottom);
            var label = child as Label;
            if (label != null)
            {
                int needed = label.GetPreferredSize(new Size(label.Width, 0)).Height;
                Check("label text is not clipped (" + size + ")", label.Height >= needed, "[" + name + "] height=" + label.Height + " needed=" + needed);
            }
            previous = child;
        }
        var list = Descendants(form).OfType<ListBox>().Single();
        Check("expression list shows at least three rows (" + size + ")", list.ClientSize.Height >= 3 * list.ItemHeight,
            "height=" + list.ClientSize.Height + " item=" + list.ItemHeight);
        var actions = Descendants(form).OfType<FlowLayoutPanel>().Single();
        foreach (var button in actions.Controls.OfType<Button>())
            Check("action button is visible (" + size + ")", button.Right <= actions.ClientSize.Width && button.Bottom <= actions.ClientSize.Height,
                "[" + button.Text + "] bottom-right=" + new Point(button.Right, button.Bottom) + " panel=" + actions.ClientSize);
    }
    [STAThread]
    static int Main()
    {
        Application.EnableVisualStyles(); Application.SetCompatibleTextRenderingDefault(false);
        string directory = Path.Combine(Path.GetTempPath(), "hanautomata-assist-layout-" + Guid.NewGuid().ToString("N"));
        using (var form = new PersonalAssistWindow(Path.Combine(directory, "missing-helper.exe"), Path.Combine(directory, "personal-expressions.json")))
        {
            // Materialize native metrics while the form remains hidden.
            foreach (Control control in Descendants(form).ToArray()) { IntPtr handle = control.Handle; }
            Layout(form);
            var scale = form.CurrentAutoScaleDimensions;
            Check("initial client dimensions follow the display scale", form.ClientSize.Width >= 760 * scale.Width / 96 - 1 && form.ClientSize.Height >= 580 * scale.Height / 96 - 1,
                "client=" + form.ClientSize + " dpi=" + scale);
            // The longest status the window writes: a failed ranking save after a copy, then a full recommendation line.
            var status = Descendants(form).OfType<Label>().Single(label => label.AccessibleName == "추천 상태");
            status.Text = "복사는 완료됐지만 순위를 저장하지 못했습니다. 추천 설정을 저장했습니다. 내 표현 추천 512개 · 복사해서 사용하세요.";
            Layout(form);
            CheckFits(form, "initial");
            form.Size = form.MinimumSize; Layout(form);
            CheckFits(form, "minimum");
            Check("process was not started", !Directory.Exists(directory), directory);
        }
        Console.WriteLine(failures == 0 ? "Personal assist layout tests passed." : failures + " personal assist layout checks failed.");
        return failures == 0 ? 0 : 1;
    }
}
