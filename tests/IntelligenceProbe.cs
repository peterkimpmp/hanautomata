using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Web.Script.Serialization;
using HanFlow;

static class IntelligenceProbe
{
    static int Main(string[] args)
    {
        var detector = new Detector(); var rows = new List<object>();
        var samples = File.ReadAllLines(args[0]).Where(line => line.Length > 0 && line[0] != '#').Select(line => line.Split('\t')).ToArray();
        var timings = new List<double>(); int correct = 0;
        foreach (var pair in samples) detector.Decide(pair[0]);
        foreach (var pair in samples)
        {
            var actual = detector.Decide(pair[0]); bool passed = actual.Text == pair[1]; if (passed) correct++;
            rows.Add(new { raw = pair[0], expected = pair[1], actual = actual.Text, passed });
            for (int i = 0; i < 50; i++)
            {
                long start = Stopwatch.GetTimestamp(); detector.Decide(pair[0]);
                timings.Add((Stopwatch.GetTimestamp() - start) * 1000.0 / Stopwatch.Frequency);
            }
        }
        // Also measure every prefix as it is typed, plus the bounded worst-case length.
        foreach (string value in samples.Select(pair => pair[0]).Concat(new[] { new string('a', 64), new string('r', 128) }))
            for (int len = 1; len <= value.Length; len++)
            {
                string prefix = value.Substring(0, len); long start = Stopwatch.GetTimestamp(); detector.Decide(prefix);
                timings.Add((Stopwatch.GetTimestamp() - start) * 1000.0 / Stopwatch.Frequency);
            }
        timings.Sort();
        var summary = new { total = samples.Length, correct, meanMs = timings.Average(), p95Ms = timings[(int)(timings.Count * .95)], maxMs = timings.Last(), samples = timings.Count,
            scope = "Fixed development regression fixture; warm engine latency on this PC, not general-language accuracy or end-to-end keyboard latency." };
        var json = new JavaScriptSerializer(); File.WriteAllText(args[1], json.Serialize(new { summary, rows }));
        Console.WriteLine(json.Serialize(summary)); return 0;
    }
}
