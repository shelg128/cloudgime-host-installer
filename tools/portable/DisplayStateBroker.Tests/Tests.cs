using System.Diagnostics;
using System.IO.Pipes;
using System.Reflection;
using System.Security.Principal;
using System.Text.Json;

namespace DisplayPrepareHelper;
internal static class DisplayStateBrokerTests
{
 static int count;
 static void Check(bool value,string name){if(!value)throw new Exception(name);count++;Console.WriteLine("PASS "+name);}
 static void Throws(Action action,string name){try{action();}catch{Check(true,name);return;}throw new Exception("Expected rejection: "+name);}
 static object? Invoke(string method, params object?[] args) => typeof(Program).GetMethod(method,BindingFlags.Static|BindingFlags.NonPublic)!.Invoke(null,args);
 static async Task Main()
 {
  foreach(var value in new[]{"", "abc", "two words", "C:\\path ending\\", "embedded\"quote"}) {
   var result=DisplayStateTransport.Quote(value);Check(result.StartsWith('"')&&result.EndsWith('"'),"quoted argument "+count);
  }
  Check(DisplayStateTransport.Quote("C:\\x\\")=="\"C:\\x\\\\\"","trailing slash escaped");
  Throws(()=>DisplayStateTransport.Quote("evil\0"),"NUL argument rejected");
  var state=new PrepareStateFile{SessionToken="claim-a",AppliedDisplay=new SavedDisplayState{DeviceName=@"\\.\DISPLAY7",Width=1920,Height=1080,Frequency=60}};
  Invoke("ValidateBrokerState",state,"claim-a");Check(true,"bound display mode accepted");
  Throws(()=>Invoke("ValidateBrokerState",state,"claim-b"),"foreign session rejected");
  state.AppliedDisplay.DeviceName="C:\\Windows\\system.ini";Throws(()=>Invoke("ValidateBrokerState",state,"claim-a"),"filesystem path rejected as display");
  state.AppliedDisplay.DeviceName=@"\\.\DISPLAY7";state.AppliedDisplay.Width=20000;Throws(()=>Invoke("ValidateBrokerState",state,"claim-a"),"unbounded mode rejected");
  if(!WindowsIdentity.GetCurrent().IsSystem){var rejected=(HelperResult)Invoke("RunStateBroker",new object?[]{new[]{"--broker","1","resize","--session-token","test"}})!;Check(!rejected.Ok&&rejected.Reason=="display_broker_requires_system","unprivileged broker denied before I/O");}
  using(var stream=new MemoryStream()) {await DisplayStateTransport.Send(stream,new { operation="read",value=(object?)null },CancellationToken.None);stream.Position=0;var value=await DisplayStateTransport.Read(stream,CancellationToken.None);Check(value.GetProperty("operation").GetString()=="read","framed message round trip");}
  foreach(var length in new[]{-1,0,DisplayStateTransport.MaxBytes+1}) {using var stream=new MemoryStream(BitConverter.GetBytes(length));Throws(()=>DisplayStateTransport.Read(stream,CancellationToken.None).GetAwaiter().GetResult(),"invalid frame rejected "+length);}
  var name="CloudgimeDisplay-"+Guid.NewGuid().ToString("N");var sid=WindowsIdentity.GetCurrent().User!;
  using(var server=DisplayStateTransport.CreateServer(name,sid)) {
   var exchange=Task.Run(()=>DisplayStateTransport.Exchange(name,Environment.ProcessId,new {operation="read",value=(object?)null}));
   await server.WaitForConnectionAsync();
   Check(DisplayStateTransport.IsChild(server,Environment.ProcessId,(uint)Process.GetCurrentProcess().SessionId),"pipe client PID/session matched");
   Check(!DisplayStateTransport.IsChild(server,Environment.ProcessId+9999,(uint)Process.GetCurrentProcess().SessionId),"foreign PID rejected");
   Check(!DisplayStateTransport.IsChild(server,Environment.ProcessId,(uint)Process.GetCurrentProcess().SessionId+9999),"foreign Windows session rejected");
   var request=await DisplayStateTransport.Read(server,CancellationToken.None);Check(request.GetProperty("operation").GetString()=="read","bound pipe request parsed");
   await DisplayStateTransport.Send(server,new{ok=true,value=new{test=true}},CancellationToken.None);var receipt=await DisplayStateTransport.Read(server,CancellationToken.None);Check(receipt.GetProperty("received").GetBoolean(),"client receipt prevents early disconnect");var response=await exchange;Check(response.GetProperty("test").GetBoolean(),"server PID checked and response returned");
  }
  var name2="CloudgimeDisplay-"+Guid.NewGuid().ToString("N");
  using(var server=DisplayStateTransport.CreateServer(name2,sid)) {
   var exchange=Task.Run(()=>{try{DisplayStateTransport.Exchange(name2,Environment.ProcessId+1,new{operation="read",value=(object?)null});return false;}catch(UnauthorizedAccessException){return true;}});
   await server.WaitForConnectionAsync();Check(await exchange,"spoofed server PID rejected");
  }
  Console.WriteLine("DISPLAY_BROKER_TESTS_PASS "+count);
 }
}
