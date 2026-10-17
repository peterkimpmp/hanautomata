using System;
using System.Collections.Generic;
using System.IO;
using System.Threading;
using Hanautomata;

class PersonalAssistTests
{
    static int checks;
    static void Check(bool value) { if(!value) throw new Exception("assist assertion "+(checks+1)); checks++; }
    static AssistReply Ask(PersonalAssistClient client,string command,string key=null,object value=null)
    {
        var fields=new Dictionary<string,object>(); if(key!=null) fields[key]=value;
        using(var done=new ManualResetEvent(false)) {
            AssistReply result=null; client.Send(command,fields,delegate(AssistReply reply) { result=reply; done.Set(); });
            if(!done.WaitOne(6000)) throw new Exception("assist response timeout"); return result;
        }
    }
    static int Main(string[] args)
    {
        string directory=Path.Combine(Path.GetTempPath(),"hanautomata-assist-client-"+Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(directory); string store=Path.Combine(directory,"personal-expressions.json");
        try {
            using(var client=new PersonalAssistClient(Path.GetFullPath(args[0]),store)) {
                var reply=Ask(client,"list"); Check(reply.ok && !reply.enabled && reply.items.Length==0);
                Check(Ask(client,"add","text","검토 후 회신드리겠습니다.").ok);
                Check(Ask(client,"set_enabled","enabled",true).ok);
                reply=Ask(client,"query","text","검토"); Check(reply.items.Length==1); string id=reply.items[0].id;
                reply=Ask(client,"copied","entry_id",id); Check(reply.ok && reply.undo_id.Length>0);
                Check(Ask(client,"undo","undo_id",reply.undo_id).items[0].copies==0);
                Check(!Ask(client,"unknown").ok);
                Check(Ask(client,"list").items.Length==1); // Protocol remains usable after an application error.
                Check(Ask(client,"delete","entry_id",id).items.Length==0);
            }
            bool rejected=false; try { PersonalAssistClient.ReadFrame(new StringReader("partial")); } catch(EndOfStreamException) { rejected=true; } Check(rejected);
            rejected=false; try { PersonalAssistClient.ReadFrame(new StringReader(new string('x',1048577)+"\n")); } catch(InvalidDataException) { rejected=true; } Check(rejected);
            using(var missing=new PersonalAssistClient(Path.Combine(directory,"missing-helper"),store)) Check(!Ask(missing,"list").ok);
            Console.WriteLine("PASS: "+checks+" C# personal-assist client assertions"); return 0;
        } finally { try { Directory.Delete(directory,true); } catch {} }
    }
}
