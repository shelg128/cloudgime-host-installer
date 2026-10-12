using System.Diagnostics;
using System.Text.Json;
using CloudgimeHostBootstrap;
internal static class HeadlessProcessTests
{
    internal static bool Worker(string[] args)
    {
        if(args.Length==0||args[0]!="--headless-child-fixture")return false;
        if(args.Length!=2)throw new InvalidDataException("FIXTURE_ARGS");
        switch(args[1])
        {
            case "stdin":
                var value=Console.ReadLine();
                Console.Write(JsonSerializer.Serialize(new{stdinReceived=value=="QA_ONLY_NOT_A_LICENSE",secretInArgs=Environment.GetCommandLineArgs().Any(a=>a.Contains("QA_ONLY_NOT_A_LICENSE",StringComparison.Ordinal)),overrideInherited=Environment.GetEnvironmentVariable("CLOUDGIME_PROVISIONING_ROOT")!=null}));break;
            case "echo":Console.Write(Console.ReadLine());break;
            case "failure":Console.Error.Write("upstream password="+Console.ReadLine());Environment.ExitCode=71;break;
            case "stdout-flood":Console.ReadLine();Console.Write(new string('x',80000));Console.Out.Flush();Thread.Sleep(15000);break;
            case "stderr-flood":Console.ReadLine();Console.Error.Write(new string('x',80000));Console.Error.Flush();Thread.Sleep(15000);break;
            case "timeout":Console.ReadLine();Thread.Sleep(15000);break;
            default:throw new InvalidDataException("FIXTURE_MODE");
        }
        return true;
    }
    internal static int Run()
    {
        const string secret="QA_ONLY_NOT_A_LICENSE";int checks=0;
        void Check(bool ok,string code){checks++;if(!ok)throw new Exception(code);}
        var exe=Environment.ProcessPath!;var prefix=Path.GetFileNameWithoutExtension(exe).Equals("dotnet",StringComparison.OrdinalIgnoreCase)?new[]{typeof(HeadlessProcessTests).Assembly.Location}:Array.Empty<string>();
        string RunChild(string mode,TimeSpan? timeout=null)=>HeadlessSetupProcess.Run(exe,prefix.Concat(new[]{"--headless-child-fixture",mode}),Environment.CurrentDirectory,secret.AsMemory(),timeout??TimeSpan.FromSeconds(6));
        var old=Environment.GetEnvironmentVariable("CLOUDGIME_PROVISIONING_ROOT");
        try
        {
            Environment.SetEnvironmentVariable("CLOUDGIME_PROVISIONING_ROOT","QA_OVERRIDE_SHOULD_NOT_ESCAPE");
            using var result=JsonDocument.Parse(RunChild("stdin"));
            Check(result.RootElement.GetProperty("stdinReceived").GetBoolean(),"stdin delivered");
            Check(!result.RootElement.GetProperty("secretInArgs").GetBoolean(),"no command-line credential");
            Check(!result.RootElement.GetProperty("overrideInherited").GetBoolean(),"override removed");
            foreach(var mode in new[]{"echo","failure","stdout-flood","stderr-flood","timeout"})
            {
                var watch=Stopwatch.StartNew();
                try{RunChild(mode,mode=="timeout"?TimeSpan.FromMilliseconds(700):TimeSpan.FromSeconds(4));throw new Exception("child failure accepted");}
                catch(InvalidDataException error){Check(error.Message=="HEADLESS_SETUP_CHILD_FAILED","fixed error");Check(error.InnerException==null&&!error.ToString().Contains(secret),"no secret exception");}
                Check(watch.Elapsed<TimeSpan.FromSeconds(9),"bounded process termination");
            }
            foreach(var unsafeSecret in new[]{"line\nsecond","line\rsecond","nul\0second",new string('a',4097)})
            {
                try{HeadlessSetupProcess.Run(exe,["--headless-child-fixture","stdin"],Environment.CurrentDirectory,unsafeSecret.AsMemory(),TimeSpan.FromSeconds(1));throw new Exception("unsafe accepted");}
                catch(InvalidDataException error){Check(error.Message=="HEADLESS_SETUP_CHILD_ARGUMENT_INVALID","unsafe input denied before child");}
            }
            try{HeadlessSetupProcess.Run(exe,["--activation-license",secret],Environment.CurrentDirectory,secret.AsMemory(),TimeSpan.FromSeconds(1));throw new Exception("secret argv accepted");}
            catch(InvalidDataException error){Check(error.Message=="HEADLESS_SETUP_CHILD_ARGUMENT_INVALID","secret argument denied");}
        }
        finally{Environment.SetEnvironmentVariable("CLOUDGIME_PROVISIONING_ROOT",old);}
        Console.WriteLine($"HEADLESS_CHILD_PROCESS_ASSERTIONS={checks}; real fixture processes, stdin only; no license/account/Windows mutation");return checks;
    }
}
