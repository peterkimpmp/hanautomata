using System;
using System.IO;
using System.Text;
using HanFlow;

static class ReferenceProbe
{
    static int Main(string[] args)
    {
        if (args.Length != 2) return 2;
        using (var output = new StreamWriter(args[1], false, new UTF8Encoding(false)))
            foreach (string input in File.ReadLines(args[0], Encoding.UTF8))
            {
                string composed = Hangul.Compose(input);
                output.WriteLine(composed + "\t" + Hangul.ToKeys(composed));
            }
        return 0;
    }
}
