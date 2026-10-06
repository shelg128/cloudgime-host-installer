using System;
using System.Diagnostics;
using System.Globalization;
using System.Text;
using System.Text.RegularExpressions;
using System.Threading;
using System.Threading.Tasks;
public static class ClockProbe {
 static readonly Regex Request=new Regex("^\\{\"action\":\"(sample|exit)\",\"id\":\"([a-f0-9]{32})\",\"seq\":([0-8])\\}$",RegexOptions.CultureInvariant);
 static void Emit(string s){Console.Out.WriteLine(s);Console.Out.Flush();}
 static long Utc(){return DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();}
 static string BoundedLine(){StringBuilder b=new StringBuilder();for(;;){int c=Console.In.Read();if(c<0)return b.Length==0?null:b.ToString();if(c==10)return b.ToString().TrimEnd('\r');if(b.Length>=200)throw new Exception("CLOCK_REQUEST_SIZE");b.Append((char)c);}}
 public static int Main(){
  using(var lifetime=new Timer(_=>Environment.Exit(3),null,20000,Timeout.Infinite)){
   Stopwatch clock=Stopwatch.StartNew();long startUtc=Utc();string session=null;
   Emit("{\"type\":\"ready\",\"schema\":1,\"role\":\"clock-only\",\"maxSamples\":8,\"maxLifetimeMs\":20000,\"idleTimeoutMs\":3000}");
   try {for(int i=0;i<9;i++){
    var task=Task.Run(()=>BoundedLine());if(!task.Wait(3000))throw new Exception("CLOCK_INPUT_TIMEOUT");string line=task.Result;if(line==null)return 0;
    long h1=Utc();double m1=clock.Elapsed.TotalMilliseconds;var m=Request.Match(line);if(!m.Success||int.Parse(m.Groups[3].Value)!=i)throw new Exception("CLOCK_REQUEST");
    if(session==null)session=m.Groups[2].Value;if(session!=m.Groups[2].Value)throw new Exception("CLOCK_ID");
    if(m.Groups[1].Value=="exit"){Emit("{\"type\":\"closed\",\"id\":\""+session+"\",\"seq\":"+i+"}");return 0;}if(i>=8)throw new Exception("CLOCK_SAMPLE_LIMIT");
    long h2=Utc();double m2=clock.Elapsed.TotalMilliseconds;if(Math.Abs(h2-startUtc-m2)>20)throw new Exception("CLOCK_STEP");
    Emit("{\"type\":\"sample\",\"id\":\""+session+"\",\"seq\":"+i.ToString(CultureInfo.InvariantCulture)+",\"receivedUtcMs\":"+h1.ToString(CultureInfo.InvariantCulture)+",\"sentUtcMs\":"+h2.ToString(CultureInfo.InvariantCulture)+",\"receivedMonoMs\":"+m1.ToString("R",CultureInfo.InvariantCulture)+",\"sentMonoMs\":"+m2.ToString("R",CultureInfo.InvariantCulture)+"}");
   }}catch(Exception e){string message=e is AggregateException?e.InnerException.Message:e.Message;string safe=message.StartsWith("CLOCK_")?message:"CLOCK_INTERNAL";Emit("{\"type\":\"error\",\"code\":\""+safe+"\"}");return 2;}
   return 0;
  }
 }
}
