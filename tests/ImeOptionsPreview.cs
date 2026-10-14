using System;
using System.IO;
using Hanautomata;

// Visible UI smoke-test host. Saves only beside this test executable, never in the user's live settings directory.
internal static class ImeOptionsPreview
{
    [STAThread]
    static int Main() { return ImeOptionsProgram.Run(Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "preview-settings.json")); }
}
