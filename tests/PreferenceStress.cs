using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Web.Script.Serialization;
using HanFlow;

static class PreferenceStress
{
    static int Main(string[] args)
    {
        int rounds = int.Parse(args[0]), failures = 0;
        string folder = Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "stress-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(folder);
        var errors = new List<object>(); var timer = Stopwatch.StartNew();
        try
        {
            string path = Path.Combine(folder, "words.json"), context = WordPreferences.ContextKey("나는");
            var words = new WordPreferences(); var store = new PreferenceStore(path, words);
            for (int i = 0; i < rounds; i++)
            {
                words.Remember("go", "go", i % 2 == 0, context); store.ScheduleSave(); words.Clear();
                bool persisted = store.FlushNow(); string code = store.LastFailureCode;
                var loaded = new WordPreferences(); var reader = new PreferenceStore(path, loaded);
                if (!persisted || loaded.Count != 0 || reader.LastError.Length != 0)
                {
                    failures++;
                    if (errors.Count < 10) errors.Add(new { iteration = i, persisted, code, count = loaded.Count, readFailed = reader.LastError.Length != 0 });
                }
            }
            store.FlushNow();
        }
        finally { Directory.Delete(folder, true); }
        string json = new JavaScriptSerializer().Serialize(new { rounds, failures, elapsedMs = timer.ElapsedMilliseconds, errors,
            scope = "Synthetic explicit-choice / queued save / reset / synchronous flush / restart-read interleavings on this Windows filesystem." });
        File.WriteAllText(args[1], json); Console.WriteLine(json); return failures == 0 ? 0 : 1;
    }
}
