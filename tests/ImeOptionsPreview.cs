using System;
using System.IO;
using System.Collections.Generic;
using System.Linq;
using System.Threading.Tasks;
using System.Web.Script.Serialization;
using System.Windows.Forms;
using Hanautomata;

// Visible UI smoke-test host. Saves only beside this test executable, never in the user's live settings directory.
internal static class ImeOptionsPreview
{
    // Entirely in-memory fixture: native helper and real learning files are never used by this preview.
    sealed class PreviewLearning : ILearningClient
    {
        readonly LearningSnapshot snapshot = new LearningSnapshot {
            version = new LearningVersion { store_id = "1234567890abcdef1234567890abcdef", generation = 1, revision = 1 },
            entries = new[] {
                new LearningWord { raw = "dkssud", keys = "dkssud", korean = true, sequence = 1, context = "", korean_weight = 1 },
                new LearningWord { raw = "hello", keys = "hello", korean = false, sequence = 2, context = "", english_weight = 1 }
            }
        };
        public Task<LearningReply> SendAsync(object request)
        {
            var serializer = new JavaScriptSerializer();
            var values = serializer.Deserialize<Dictionary<string, object>>(LearningClient.Encode(request));
            string command = (string)values["command"];
            if (command == "reset") snapshot.entries = new LearningWord[0];
            if (command == "forget") snapshot.entries = snapshot.entries.Where(word => word.raw != (string)values["raw"] || word.keys != (string)values["keys"]).ToArray();
            if (command != "snapshot") { snapshot.version.generation++; snapshot.version.revision++; }
            return Task.FromResult(serializer.Deserialize<LearningReply>(serializer.Serialize(new LearningReply { protocol = 1, ok = true, snapshot = snapshot })));
        }
    }
    [STAThread]
    static int Main()
    {
        Application.EnableVisualStyles(); Application.SetCompatibleTextRenderingDefault(false);
        using (var form = new ImeOptionsWindow(Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "preview-settings.json"), new PreviewLearning()))
        {
            form.Text += " · 화면 검수본 · 가상 학습 2개";
            Application.Run(form);
        }
        return 0;
    }
}
