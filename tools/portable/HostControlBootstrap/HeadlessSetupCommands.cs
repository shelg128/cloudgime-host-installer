using System.Text.Json;

namespace CloudgimeHostBootstrap;

internal static class HeadlessSetupCommands
{
    internal static bool Handles(string[] args)=>args.Length>0&&args[0] is
        "validate-headless-package" or "validate-universal-bundle" or "headless-setup-plan" or "headless-setup-install" or "headless-setup-activate";
    internal static int Run(string[] args)
    {
        // Keep these errors separate from legacy bootstrap exception logging:
        // incoming paths/output are never included in a public diagnostic.
        try
        {
            if(args[0]=="headless-setup-install")
            {
                if(args.Length is not(2 or 3)||args.Length==3&&args[2]!="--silent")throw new InvalidDataException("HEADLESS_COMMAND_INVALID");
                var chars=new char[4097];int count=0;
                try
                {
                    int value;while((value=Console.In.Read())!=-1&&value!='\n')
                    {if(value=='\r')continue;if(count>=4096)throw new InvalidDataException("HEADLESS_LICENSE_INVALID");chars[count++]=(char)value;}
                    HeadlessFreshInstall.Run(args[1],chars.AsMemory(0,count));return 0;
                }
                finally{Array.Clear(chars);}
            }
            if(args[0]=="headless-setup-activate")throw new InvalidDataException("HEADLESS_USE_INSTALL_RESUME");
            if(args[0]=="validate-universal-bundle")
            {
                // This is the installer-builder contract. Keep the root named so
                // it cannot be confused with a future installer option.
                if(args.Length!=4||args[1]!="--silent"||args[2]!="--bundle-root")
                    throw new InvalidDataException("HEADLESS_COMMAND_INVALID");
                var universalRoot=Path.GetFullPath(args[3]);
                UniversalBundleContract.Validate(universalRoot);
                Console.WriteLine(JsonSerializer.Serialize(new
                {
                    packageIntegrityVerified=true,
                    hostControlGuiIncluded=true,
                    mode="UNIVERSAL_BUNDLE_VALIDATED",
                    applicationFilesChanged=false,
                    licenseClaimed=false,
                    windowsAccountCreated=false,
                    autoLogonArmed=false,
                    restartRequested=false,
                    readyForRental=false,
                    installationEnabled=false
                }));
                return 0;
            }
            if(args.Length is not(2 or 3)||args.Length==3&&args[2]!="--silent")
                throw new InvalidDataException("HEADLESS_COMMAND_INVALID");
            var root=Path.GetFullPath(args[1]);
            HeadlessBundleContract.Validate(root);
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
