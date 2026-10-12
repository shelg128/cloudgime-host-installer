using System.Diagnostics;
using System.Text;
namespace CloudgimeHostBootstrap;

internal static class HeadlessSetupProcess
{
    // Never log args/stdout/stderr. A separate typed caller must parse any public
    // result; a process success code is not proof of provisioning or readiness.
    internal static string Run(string image,IEnumerable<string> args,string workingDirectory,
        ReadOnlyMemory<char> secret,TimeSpan timeout)
    {
        if(timeout<TimeSpan.FromMilliseconds(100)||timeout>TimeSpan.FromMinutes(4)
            ||secret.Length>4096||secret.Span.ContainsAny('\r','\n','\0'))
            throw new InvalidDataException("HEADLESS_SETUP_CHILD_ARGUMENT_INVALID");
        var info=new ProcessStartInfo(image){UseShellExecute=false,CreateNoWindow=true,WorkingDirectory=workingDirectory,
            RedirectStandardInput=true,RedirectStandardOutput=true,RedirectStandardError=true};
        foreach(var arg in args)
        {
            if(arg is null||arg.IndexOf('\0')>=0||!secret.IsEmpty&&arg.AsSpan().Contains(secret.Span,StringComparison.Ordinal))
                throw new InvalidDataException("HEADLESS_SETUP_CHILD_ARGUMENT_INVALID");
            info.ArgumentList.Add(arg);
        }
        // Do not inherit redirectable provisioning roots or profiler/startup hooks.
        foreach(var key in info.Environment.Keys.Where(k=>k.StartsWith("CLOUDGIME_",StringComparison.OrdinalIgnoreCase)
            ||k.StartsWith("COR_",StringComparison.OrdinalIgnoreCase)||k.StartsWith("CORECLR_",StringComparison.OrdinalIgnoreCase)
            ||k.Equals("DOTNET_STARTUP_HOOKS",StringComparison.OrdinalIgnoreCase)).ToArray())info.Environment.Remove(key);
        using var process=new Process{StartInfo=info};using var deadline=new CancellationTokenSource(timeout);
        Task<string>? output=null;Task? errors=null;
        try
        {
            if(!process.Start())throw new IOException();
            output=ReadBounded(process.StandardOutput,65536,deadline);
            errors=DrainBounded(process.StandardError,65536,deadline);
            if(!secret.IsEmpty)
            {
                process.StandardInput.WriteAsync(secret,deadline.Token).GetAwaiter().GetResult();
                process.StandardInput.WriteLineAsync(ReadOnlyMemory<char>.Empty,deadline.Token).GetAwaiter().GetResult();
            }
            process.StandardInput.Close();
            process.WaitForExitAsync(deadline.Token).GetAwaiter().GetResult();
            var text=output.GetAwaiter().GetResult();errors.GetAwaiter().GetResult();
            if(process.ExitCode!=0||!secret.IsEmpty&&text.AsSpan().Contains(secret.Span,StringComparison.Ordinal))
                throw new InvalidDataException("HEADLESS_SETUP_CHILD_FAILED");
            return text;
        }
        catch
        {
            deadline.Cancel();
            try{if(!process.HasExited)process.Kill(true);}catch{}
            try{process.WaitForExit(5000);}catch{}
            // Observe reader task exceptions. Never let raw output escape as an
            // inner exception (it could include the activation token).
            try{output?.GetAwaiter().GetResult();}catch{}
            try{errors?.GetAwaiter().GetResult();}catch{}
            throw new InvalidDataException("HEADLESS_SETUP_CHILD_FAILED");
        }
    }
    private static async Task<string> ReadBounded(StreamReader reader,int maximum,CancellationTokenSource deadline)
    {
        var text=new StringBuilder();var buffer=new char[2048];
        try
        {
            int n;while((n=await reader.ReadAsync(buffer.AsMemory(),deadline.Token))>0)
            {if(text.Length+n>maximum){deadline.Cancel();throw new IOException();}text.Append(buffer,0,n);}
            return text.ToString();
        }
        finally{Array.Clear(buffer);}
    }
    private static async Task DrainBounded(StreamReader reader,int maximum,CancellationTokenSource deadline)
    {
        var buffer=new char[2048];var count=0;
        try
        {
            int n;while((n=await reader.ReadAsync(buffer.AsMemory(),deadline.Token))>0)
            {count+=n;if(count>maximum){deadline.Cancel();throw new IOException();}}
        }
        finally{Array.Clear(buffer);}
    }
}
