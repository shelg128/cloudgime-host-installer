using System.Diagnostics;
using System.Security.Principal;
using System.Text.Json.Nodes;
namespace CloudgimeHostBootstrap;

// Experimental installed-layout adapter. Deliberately unavailable in legacy GUI
// installs; requires an administrator-owned marker bound to immutable payload.
internal sealed class WindowsHeadlessSetupPlatform : IHeadlessSetupPlatform
{
    private readonly string _bundle,_helper,_root,_operator,_hash;
    // Deliberately factory-less until executable hashes, installation ownership,
    // partial-provision recovery and backend freshness are pinned end-to-end.
    // No production caller may instantiate this adapter in milestone 1.
    private WindowsHeadlessSetupPlatform(string bundle,string helper,string owner,string hash)
    {_bundle=bundle;_helper=helper;_root=UniversalMachineBoundary.ProvisioningRoot;_operator=owner;_hash=hash;}
    public void RequireCurrent(HeadlessSetupState state)
    {
        UniversalMachineBoundary.RequireAdminConsole();
        using var me=WindowsIdentity.GetCurrent();if(me.User?.Value!=_operator||state.OperatorSid!=_operator||state.PackageHash!=_hash)throw new InvalidDataException("HEADLESS_OPERATOR_CHANGED");
        ManagedNodeUpgradeGuard.RequireUnmanaged(_root);
        foreach(var name in new[]{"SentinelNodeBroker","Sentinel Launcher","SentinelHelper","CloudGimeRentalShell","streamer"})
            if(Process.GetProcessesByName(name).Length>0)throw new InvalidDataException("HEADLESS_ACTIVE_RUNTIME_DENIED");
        foreach(var p in new[]{_bundle,Path.GetDirectoryName(_helper)!})UniversalMachineBoundary.RequireProtected(p,true);
        // Hashes are rechecked by a reviewed installed-layout boundary before this
        // adapter is exposed to production. Until then it is fixture/build-only.
        if(Environment.GetEnvironmentVariable("CLOUDGIME_PROVISIONING_ROOT") is not null)throw new InvalidDataException("HEADLESS_PROVISIONING_OVERRIDE_DENIED");
    }
    public bool IsComplete(string step,HeadlessSetupState state)
    {
        RequireCurrent(state);
        if(step=="PROVISION_NODE")
        {
            var manifest=Path.Combine(_root,"manifest.json");if(!File.Exists(manifest))return false;
            UniversalMachineBoundary.RequireProtected(manifest);var m=UniversalBundleContract.ParseObject(manifest);
            if((int?)m["schemaVersion"]!=2||(string?)m["status"]!="PROVISIONED_WAITING_HEARTBEATS"||(string?)m["controlPlaneUrl"]!="https://cloudgime.my.id"
                ||string.IsNullOrWhiteSpace((string?)m["binding"]?["pcId"]))throw new InvalidDataException("HEADLESS_NODE_MANIFEST_INVALID");
            foreach(var file in new[]{"machine-identity.dpapi","host_app.credential.dpapi","sentinel.credential.dpapi","installer_renewal.credential.dpapi"})UniversalMachineBoundary.RequireProtected(Path.Combine(_root,file));
            return true;
        }
        if(step=="PIN_OPERATOR")
        {
            var pin=Path.Combine(_root,"shared-gaming-desktop.identity.json");if(!File.Exists(pin))return false;
            UniversalMachineBoundary.RequireProtected(pin);var p=UniversalBundleContract.ParseObject(pin);
            if((string?)p["desktopUserSid"]!=_operator)throw new InvalidDataException("HEADLESS_OWNER_PIN_CHANGED");return true;
        }
        if(step=="CREATE_DISABLED_ACCOUNTS")
        {
            var identity=Path.Combine(_root,"rental-desktop.identity.json");if(!File.Exists(identity))return false;
            var output=HeadlessSetupProcess.Run(_helper,["--headless-rental-account-status"],Path.GetDirectoryName(_helper)!,ReadOnlyMemory<char>.Empty,TimeSpan.FromSeconds(30));
            var result=JsonNode.Parse(output)?.AsObject()??throw new InvalidDataException("HEADLESS_RENTER_STATUS_INVALID");
            if((bool?)result["verified"]!=true||(bool?)result["disabled"]!=true||(string?)result["operatorSid"]!=_operator)throw new InvalidDataException("HEADLESS_RENTER_STATUS_INVALID");return true;
        }
        throw new InvalidDataException("HEADLESS_SETUP_STEP_INVALID");
    }
    public void Execute(string step,HeadlessSetupState state,ReadOnlyMemory<char> activationLicense)
    {
        RequireCurrent(state);
        switch(step)
        {
            case "PROVISION_NODE":
                if(activationLicense.IsEmpty||activationLicense.Length>4096||activationLicense.Span.ContainsAny('\r','\n','\0'))throw new InvalidDataException("HEADLESS_LICENSE_REQUIRED");
                HeadlessSetupProcess.Run(Path.Combine(_bundle,"host-installer.exe"),["--bundle-root",_bundle,"provision","--profile","always_on_host","--api-base-url","https://cloudgime.my.id"],_bundle,activationLicense,TimeSpan.FromSeconds(75));
                break;
            case "PIN_OPERATOR":
                HeadlessSetupProcess.Run(_helper,["--pin-shared-gaming-desktop"],Path.GetDirectoryName(_helper)!,ReadOnlyMemory<char>.Empty,TimeSpan.FromSeconds(30));break;
            case "CREATE_DISABLED_ACCOUNTS":
                // The first automatic-renter milestone does not require creating
                // another administrator account or enabling the current operator.
                HeadlessSetupProcess.Run(_helper,["--provision-rental-desktop"],Path.GetDirectoryName(_helper)!,ReadOnlyMemory<char>.Empty,TimeSpan.FromSeconds(120));break;
            default:throw new InvalidDataException("HEADLESS_SETUP_STEP_INVALID");
        }
    }
}
