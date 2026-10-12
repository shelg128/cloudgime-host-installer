using System.Text.Json;

namespace CloudgimeHostBootstrap;

internal static class HeadlessSetupCommands
{
    internal static bool Handles(string[] args)=>args.Length>0&&args[0] is
        "validate-headless-package" or "headless-setup-plan" or "headless-setup-install" or "headless-setup-activate";
    internal static int Run(string[] args)
    {
        // Keep these errors separate from legacy bootstrap exception logging:
        // incoming paths/output are never included in a public diagnostic.
        try
        {
            if(args[0] is "headless-setup-install" or "headless-setup-activate")
                throw new InvalidDataException("HEADLESS_AUTOMATIC_INSTALL_NOT_QUALIFIED");
            if(args.Length is not(2 or 3)||args.Length==3&&args[2]!="--silent")
                throw new InvalidDataException("HEADLESS_COMMAND_INVALID");
            var root=Path.GetFullPath(args[1]);HeadlessBundleContract.Validate(root);
            Console.WriteLine(JsonSerializer.Serialize(new
            {
                packageIntegrityVerified=true,
                hostControlGuiIncluded=false,
                mode="HEADLESS_SETUP_FOUNDATION_ONLY",
                applicationFilesChanged=false,
                licenseClaimed=false,
                windowsAccountCreated=false,
                autoLogonArmed=false,
                restartRequested=false,
                readyForRental=false,
                installationEnabled=false,
                implementedCoordinatorSteps=HeadlessSetupCoordinator.Steps,
                remainingGates=new[]{"TRANSACTIONAL_FRESH_INSTALL","PINNED_PRIVILEGED_ADAPTER",
                    "FIRST_BOOT_LOCKED_SHELL","AUTOLOGON_RECOVERY_INTEGRATION","BACKEND_AVAILABILITY_VERIFICATION"}
            }));
            return 0;
        }
        catch(Exception e)
        {
            var code=e is InvalidDataException&&System.Text.RegularExpressions.Regex.IsMatch(e.Message,@"\A[A-Z0-9_]{1,100}\z")
                ?e.Message:"HEADLESS_PACKAGE_OR_COMMAND_REJECTED";
            Console.Error.WriteLine(code);return 53;
        }
    }
}
