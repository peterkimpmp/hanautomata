using System;
using System.IO;
using System.Windows.Forms;
using Hanautomata;

// Visible UI smoke-test host. Saves only beside this test executable, never in the user's live settings directory.
internal static class ImeOptionsPreview
{
    [STAThread]
    static int Main()
    {
        Application.EnableVisualStyles(); Application.SetCompatibleTextRenderingDefault(false);
        using (var form = new ImeOptionsWindow(Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "preview-settings.json")))
        {
            form.Text += " · 화면 검수본";
            Application.Run(form);
        }
        return 0;
    }
}
