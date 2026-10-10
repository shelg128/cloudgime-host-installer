using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Security.AccessControl;
using System.Security.Principal;
using System.Text.Json;
using System.Text.RegularExpressions;

namespace DisplayPrepareHelper;
internal static partial class Program
{
    // Windows forbids different Terminal Services sessions in the same job. The web-server
    // inherits supervisor's service-session job. Task Scheduler launches this narrow broker
    // outside it; the renter worker is still contained by its own KILL_ON_JOB_CLOSE job.
    // Never enable BREAKAWAY_OK/SILENT_BREAKAWAY_OK on the supervisor job.
    private static string BrokerTaskDirectory(string id)
    {
        if (!Regex.IsMatch(id, "^[a-f0-9]{32}$")) throw new ArgumentException("Invalid broker request id.");
        return Path.Combine(AppContext.BaseDirectory, "display-broker-requests", id);
    }
    private static HelperResult RunScheduledBroker(string[] args)
    {
        if (!WindowsIdentity.GetCurrent().IsSystem) return Failure("display_broker_requires_system");
        if (args.Length < 4 || args[0] != "--broker" || !uint.TryParse(args[1], out var session) || session == 0)
            return Failure("invalid_interactive_session");
        var options = ParseArguments(args.Skip(2).ToArray());
        if (options.Command is not (CommandKind.Prepare or CommandKind.Resize or CommandKind.Restore or CommandKind.ProjectDisplay))
            return Failure("broker_action_not_allowed");
        if (string.IsNullOrWhiteSpace(options.SessionToken) || options.SessionToken.Length > 512)
            return Failure("broker_session_required");
        string id = Guid.NewGuid().ToString("N"), name = "CloudgimeDisplayBroker-" + id;
        string directory = BrokerTaskDirectory(id), output = Path.Combine(directory, "result.json");
        object? serviceObject=null, folderObject=null, registeredObject=null, runningObject=null;
        try {
            // Parent directory inherits the administrator-only mutation ACL of the trusted helper root.
            Directory.CreateDirectory(directory);
            var security = new System.Security.AccessControl.DirectorySecurity();
            security.SetAccessRuleProtection(true, false);
            foreach (var sid in new[] { WellKnownSidType.LocalSystemSid, WellKnownSidType.BuiltinAdministratorsSid })
                security.AddAccessRule(new System.Security.AccessControl.FileSystemAccessRule(new SecurityIdentifier(sid,null),
                    System.Security.AccessControl.FileSystemRights.FullControl,
                    System.Security.AccessControl.InheritanceFlags.ContainerInherit | System.Security.AccessControl.InheritanceFlags.ObjectInherit,
                    System.Security.AccessControl.PropagationFlags.None, System.Security.AccessControl.AccessControlType.Allow));
            new DirectoryInfo(directory).SetAccessControl(security);
            serviceObject = Activator.CreateInstance(Type.GetTypeFromProgID("Schedule.Service")!);
            dynamic service = serviceObject!; service.Connect();
            folderObject = service.GetFolder("\\"); dynamic folder = folderObject;
            dynamic definition = service.NewTask(0);
            definition.RegistrationInfo.Description = "Single bounded CloudGime display state operation";
            definition.Principal.UserId = "S-1-5-18";
            definition.Principal.LogonType = 5; // TASK_LOGON_SERVICE_ACCOUNT
            definition.Principal.RunLevel = 1;
            definition.Settings.Enabled = true;
            definition.Settings.Hidden = true;
            definition.Settings.DisallowStartIfOnBatteries = false;
            definition.Settings.StopIfGoingOnBatteries = false;
            definition.Settings.ExecutionTimeLimit = "PT90S";
            definition.Settings.MultipleInstances = 2; // Ignore duplicate start
            dynamic action = definition.Actions.Create(0);
            action.Path = Environment.ProcessPath!;
            action.WorkingDirectory = AppContext.BaseDirectory;
            action.Arguments = string.Join(" ", new[] { "--broker-worker", id }.Concat(args.Skip(1)).Select(DisplayStateTransport.Quote));
            registeredObject = folder.RegisterTaskDefinition(name, definition, 2, "SYSTEM", null, 5, null); // create, never overwrite
            dynamic registered = registeredObject;
            runningObject = registered.Run(null);
            var timer = Stopwatch.StartNew();
            while (timer.Elapsed < TimeSpan.FromSeconds(85)) {
                if (File.Exists(output)) {
                    var bytes=File.ReadAllBytes(output);
                    if (bytes.Length > DisplayStateTransport.MaxBytes) return Failure("display_broker_result_too_large");
                    var result=JsonSerializer.Deserialize<HelperResult>(bytes,JsonOptions);
                    return result ?? Failure("display_broker_result_invalid");
                }
                Thread.Sleep(100);
            }
            return Failure("display_broker_task_timeout");
        } catch (Exception error) {
            Console.Error.WriteLine("Display broker task failed: " + error.GetType().Name + " hresult=" + error.HResult);
            return Failure("display_broker_scheduler_failed");
        } finally {
            if (registeredObject != null) { try { ((dynamic)registeredObject).Stop(0); } catch { } }
            if (folderObject != null) { try { ((dynamic)folderObject).DeleteTask(name,0); } catch { } }
            foreach(var item in new[] {runningObject,registeredObject,folderObject,serviceObject})
                if(item!=null && Marshal.IsComObject(item)) Marshal.FinalReleaseComObject(item);
            // Keep trusted result as crash diagnostic, not in client-writable storage. No token is written.
        }
    }
    private static int RunBrokerTaskWorker(string[] args)
    {
        if (!WindowsIdentity.GetCurrent().IsSystem || args.Length < 5 || args[0] != "--broker-worker") return 1;
        var directory = BrokerTaskDirectory(args[1]);
        if (!Directory.Exists(directory) || (File.GetAttributes(directory) & FileAttributes.ReparsePoint) != 0) return 1;
        HelperResult result;
        try { result=RunStateBroker(new[]{"--broker"}.Concat(args.Skip(2)).ToArray()); }
        catch(Exception error) {
            result=Failure(error is System.ComponentModel.Win32Exception win32
                ? "display_broker_failed_win32_"+win32.NativeErrorCode+"_"+(error.Data["stage"] as string ?? "unknown")
                : "display_broker_failed_"+error.GetType().Name.ToLowerInvariant());
        }
        var temp=Path.Combine(directory,"result.tmp");
        File.WriteAllText(temp,JsonSerializer.Serialize(result,JsonOptions));
        File.Move(temp,Path.Combine(directory,"result.json"));
        return result.Ok?0:1;
    }
}
