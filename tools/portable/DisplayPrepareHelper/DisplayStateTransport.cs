using System.Diagnostics;
using System.IO.Pipes;
using System.Runtime.InteropServices;
using System.Security.AccessControl;
using System.Security.Principal;
using System.Text;
using System.Text.Json;
using Microsoft.Win32.SafeHandles;

namespace DisplayPrepareHelper;

// One invocation, one child PID, one operation. No generic privileged filesystem or command API.
internal static class DisplayStateTransport
{
    internal const int MaxBytes = 512 * 1024;
    internal static JsonElement Exchange(string pipeName, int expectedServerPid, object request)
    {
        using var pipe = new NamedPipeClientStream(".", pipeName, PipeDirection.InOut, PipeOptions.Asynchronous);
        pipe.Connect(10000);
        if (!Native.GetNamedPipeServerProcessId(pipe.SafePipeHandle, out var pid) || pid != expectedServerPid)
            throw new UnauthorizedAccessException("Display state broker identity mismatch.");
        using var deadline = new CancellationTokenSource(TimeSpan.FromSeconds(15));
        Send(pipe, request, deadline.Token).GetAwaiter().GetResult();
        var reply = Read(pipe, deadline.Token).GetAwaiter().GetResult();
        // Server must not Disconnect() until the client has consumed its response.
        Send(pipe, new { received = true }, deadline.Token).GetAwaiter().GetResult();
        if (!reply.GetProperty("ok").GetBoolean()) throw new InvalidOperationException("Display state broker rejected operation.");
        return reply.GetProperty("value").Clone();
    }
    internal static async Task Send(Stream stream, object value, CancellationToken ct)
    {
        var bytes = JsonSerializer.SerializeToUtf8Bytes(value);
        if (bytes.Length > MaxBytes) throw new InvalidDataException("Display state response too large.");
        await stream.WriteAsync(BitConverter.GetBytes(bytes.Length), ct);
        await stream.WriteAsync(bytes, ct);
        await stream.FlushAsync(ct);
    }
    internal static async Task<JsonElement> Read(Stream stream, CancellationToken ct)
    {
        var header = new byte[4]; await stream.ReadExactlyAsync(header, ct);
        var count = BitConverter.ToInt32(header);
        if (count is <= 0 or > MaxBytes) throw new InvalidDataException("Invalid display broker message size.");
        var bytes = new byte[count]; await stream.ReadExactlyAsync(bytes, ct);
        using var doc = JsonDocument.Parse(bytes, new JsonDocumentOptions { MaxDepth = 32 });
        return doc.RootElement.Clone();
    }
    internal static NamedPipeServerStream CreateServer(string name, SecurityIdentifier childSid)
    {
        var security = new PipeSecurity(); security.SetAccessRuleProtection(true, false);
        security.AddAccessRule(new PipeAccessRule(new SecurityIdentifier(WellKnownSidType.NetworkSid, null), PipeAccessRights.FullControl, AccessControlType.Deny));
        security.AddAccessRule(new PipeAccessRule(new SecurityIdentifier(WellKnownSidType.LocalSystemSid, null), PipeAccessRights.FullControl, AccessControlType.Allow));
        security.AddAccessRule(new PipeAccessRule(childSid, PipeAccessRights.ReadWrite, AccessControlType.Allow));
        return NamedPipeServerStreamAcl.Create(name, PipeDirection.InOut, 1, PipeTransmissionMode.Byte,
            PipeOptions.Asynchronous | PipeOptions.FirstPipeInstance, 16384, 16384, security);
    }
    internal static bool IsChild(NamedPipeServerStream pipe, int expectedPid, uint expectedSession) =>
        Native.GetNamedPipeClientProcessId(pipe.SafePipeHandle, out var pid) && pid == expectedPid &&
        Native.ProcessIdToSessionId(pid, out var session) && session == expectedSession;

    internal sealed class Child : IDisposable
    {
        internal readonly int Pid;
        private IntPtr handle;
        private IntPtr job;
        internal Child(int pid, IntPtr handle, IntPtr job) { Pid = pid; this.handle = handle; this.job = job; }
        internal bool Exited => Native.WaitForSingleObject(handle, 0) == 0;
        internal int ExitCode { get { Native.GetExitCodeProcess(handle, out var code); return (int)code; } }
        public void Dispose() { if (job != IntPtr.Zero) { Native.CloseHandle(job); job = IntPtr.Zero; } if (handle == IntPtr.Zero) return; if (!Exited) Native.TerminateProcess(handle, 1); Native.CloseHandle(handle); handle = IntPtr.Zero; }
    }
    private static System.ComponentModel.Win32Exception NativeFailure(string stage)
    {
        var error = new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "Display worker native operation failed.");
        error.Data["stage"] = stage;
        return error;
    }
    internal static (Child child, SecurityIdentifier sid) StartChild(uint session, string[] args, string pipeName)
    {
        if (session == 0 || !Native.WTSQueryUserToken(session, out var queriedToken)) throw new UnauthorizedAccessException("Interactive display token unavailable.");
        IntPtr token;
        try {
            if (!Native.DuplicateTokenEx(queriedToken, 0x02000000, IntPtr.Zero, 2, 1, out token))
                throw NativeFailure("duplicate_primary_token");
        } finally { Native.CloseHandle(queriedToken); }
        try {
            using var identity = new WindowsIdentity(token); var sid = identity.User ?? throw new UnauthorizedAccessException();
            var exe = Environment.ProcessPath ?? throw new InvalidOperationException("Helper executable unavailable.");
            var argv = new[] { exe, "--state-broker", pipeName, "--broker-pid", Environment.ProcessId.ToString() }.Concat(args);
            var command = new StringBuilder(string.Join(" ", argv.Select(Quote)));
            var info = new Native.Startup { cb = Marshal.SizeOf<Native.Startup>(), desktop = "winsta0\\default", flags = 1, show = 0 };
            IntPtr environment = IntPtr.Zero;
            if (!Native.CreateEnvironmentBlock(out environment, token, false))
                throw NativeFailure("environment");
            Native.ProcessInfo p;
            try {
                const uint flags = 0x08000404; // no window, Unicode environment, initially suspended
                if (!Native.CreateProcessAsUserW(token, exe, command, IntPtr.Zero, IntPtr.Zero, false, flags, environment, AppContext.BaseDirectory, ref info, out p))
                    throw NativeFailure("launch_as_user");
            } finally { Native.DestroyEnvironmentBlock(environment); }
            IntPtr job = IntPtr.Zero;
            try {
                if (!Native.ProcessIdToSessionId(p.pid, out var actualSession) || actualSession != session)
                    throw new UnauthorizedAccessException("Display worker not in requested interactive session.");
                Console.Error.WriteLine("Display broker child launched in verified session=" + actualSession);
                job = Native.CreateJobObjectW(IntPtr.Zero, null);
                if (job == IntPtr.Zero) throw NativeFailure("job_create");
                var limits = new Native.ExtendedLimits { basic = new Native.BasicLimits { flags = 0x2000 } };
                if (!Native.SetInformationJobObject(job, 9, ref limits, (uint)Marshal.SizeOf<Native.ExtendedLimits>()))
                    throw NativeFailure("job_limits");
                if (!Native.AssignProcessToJobObject(job, p.process))
                    throw NativeFailure("job_assign");
                if (Native.ResumeThread(p.thread) == uint.MaxValue)
                    throw NativeFailure("resume");
                return (new Child((int)p.pid, p.process, job), sid);
            } catch {
                Native.TerminateProcess(p.process, 1);
                Native.CloseHandle(p.process);
                if (job != IntPtr.Zero) Native.CloseHandle(job);
                throw;
            } finally { Native.CloseHandle(p.thread); }
        } finally { Native.CloseHandle(token); }
    }
    internal static SecurityIdentifier SessionSid(uint session)
    {
        if (session == 0 || !Native.WTSQueryUserToken(session, out var token)) throw new UnauthorizedAccessException("Interactive display token unavailable.");
        try { using var id = new WindowsIdentity(token); return id.User ?? throw new UnauthorizedAccessException(); }
        finally { Native.CloseHandle(token); }
    }
    internal static string Quote(string s)
    {
        if (s.IndexOf('\0') >= 0) throw new ArgumentException("NUL argument");
        var b = new StringBuilder("\""); int slash = 0;
        foreach (char c in s) { if (c == '\\') { slash++; continue; } if (c == '"') b.Append('\\', slash * 2 + 1); else b.Append('\\', slash); b.Append(c); slash = 0; }
        return b.Append('\\', slash * 2).Append('"').ToString();
    }
    private static class Native
    {
        [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] internal struct Startup { public int cb; public string? reserved, desktop, title; public uint x,y,w,h,xc,yc,fill,flags; public ushort show,cb2; public IntPtr reserved2,input,output,error; }
        [StructLayout(LayoutKind.Sequential)] internal struct ProcessInfo { public IntPtr process,thread; public uint pid,tid; }
        [StructLayout(LayoutKind.Sequential)] internal struct BasicLimits { public long processTime,jobTime; public uint flags; public UIntPtr minWorkingSet,maxWorkingSet; public uint activeProcessLimit; public UIntPtr affinity; public uint priority,scheduling; }
        [StructLayout(LayoutKind.Sequential)] internal struct IoCounters { public ulong readOps,writeOps,otherOps,readBytes,writeBytes,otherBytes; }
        [StructLayout(LayoutKind.Sequential)] internal struct ExtendedLimits { public BasicLimits basic;public IoCounters io;public UIntPtr processMemory,jobMemory,peakProcess,peakJob; }
        [DllImport("kernel32.dll", CharSet=CharSet.Unicode,SetLastError=true)] internal static extern IntPtr CreateJobObjectW(IntPtr attributes,string? name);
        [DllImport("kernel32.dll",SetLastError=true)] internal static extern bool SetInformationJobObject(IntPtr job,int type,ref ExtendedLimits info,uint size);
        [DllImport("kernel32.dll",SetLastError=true)] internal static extern bool AssignProcessToJobObject(IntPtr job,IntPtr process);
        [DllImport("kernel32.dll",SetLastError=true)] internal static extern uint ResumeThread(IntPtr thread);
        [DllImport("kernel32.dll", SetLastError=true)] internal static extern bool GetNamedPipeClientProcessId(SafePipeHandle pipe, out uint pid);
        [DllImport("kernel32.dll", SetLastError=true)] internal static extern bool GetNamedPipeServerProcessId(SafePipeHandle pipe, out uint pid);
        [DllImport("kernel32.dll")] internal static extern bool ProcessIdToSessionId(uint pid,out uint session);
        [DllImport("wtsapi32.dll", SetLastError=true)] internal static extern bool WTSQueryUserToken(uint session, out IntPtr token);
        [DllImport("advapi32.dll", CharSet=CharSet.Unicode, SetLastError=true)] internal static extern bool CreateProcessAsUserW(IntPtr token,string app,StringBuilder args,IntPtr pa,IntPtr ta,bool inherit,uint flags,IntPtr env,string cwd,ref Startup si,out ProcessInfo pi);
        [DllImport("advapi32.dll",SetLastError=true)] internal static extern bool DuplicateTokenEx(IntPtr token,uint access,IntPtr attributes,int level,int type,out IntPtr duplicated);
        [DllImport("userenv.dll", SetLastError=true)] internal static extern bool CreateEnvironmentBlock(out IntPtr environment,IntPtr token,bool inherit);
        [DllImport("userenv.dll")] internal static extern bool DestroyEnvironmentBlock(IntPtr environment);
        [DllImport("kernel32.dll")] internal static extern bool CloseHandle(IntPtr h);
        [DllImport("kernel32.dll")] internal static extern uint WaitForSingleObject(IntPtr h,uint ms);
        [DllImport("kernel32.dll")] internal static extern bool GetExitCodeProcess(IntPtr h,out uint code);
        [DllImport("kernel32.dll")] internal static extern bool TerminateProcess(IntPtr h,uint code);
    }
}
