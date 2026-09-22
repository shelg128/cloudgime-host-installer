using System.Text.Json.Nodes;

namespace CloudgimeHostBootstrap;

// Pure policy shared by the installer and the regression harness. No OS mutation.
internal static class UniversalInstallPolicy
{
    internal static bool IsUniversal(JsonObject manifest)
        => string.Equals(manifest["bundleMode"]?.GetValue<string>(), "universal", StringComparison.OrdinalIgnoreCase);

    internal static void CheckReset(bool existingIdentity, bool reset, bool overwrite)
    {
        if (existingIdentity && (reset || overwrite))
            throw new InvalidOperationException("EXISTING_NODE_IDENTITY: reinstall/repair must preserve Node identity. Decommission through the control plane before fresh installation.");
    }

    internal static bool IsActivated(JsonObject? state)
    {
        if (state is null || !string.Equals((string?)state["ActivationState"], "activated", StringComparison.OrdinalIgnoreCase)) return false;
        return !string.IsNullOrWhiteSpace((string?)state["HostId"])
            && (!string.IsNullOrWhiteSpace((string?)state["ActivationRecordId"]) || !string.IsNullOrWhiteSpace((string?)state["ApplicationActivationId"]))
            && (!string.IsNullOrWhiteSpace((string?)state["CredentialRef"]) || !string.IsNullOrWhiteSpace((string?)state["RuntimeToken"]));
    }

    internal static bool Preserve(string relativePath)
    {
        var p = relativePath.Replace('/', '\\');
        return p.StartsWith("provisioning\\", StringComparison.OrdinalIgnoreCase)
            || p.StartsWith("keeper-tunnel\\data\\", StringComparison.OrdinalIgnoreCase)
            || p.StartsWith("moonlight\\server\\sunshine-shared\\", StringComparison.OrdinalIgnoreCase)
            || p.StartsWith("sunshine\\config\\", StringComparison.OrdinalIgnoreCase)
            || p.StartsWith("sunshine-legacy\\config\\", StringComparison.OrdinalIgnoreCase)
            || p.EndsWith(".dpapi", StringComparison.OrdinalIgnoreCase)
            || p.Equals("PUBLIC_URL.txt", StringComparison.OrdinalIgnoreCase)
            || p.Equals("moonlight\\server\\host_provisioning_secret.xml", StringComparison.OrdinalIgnoreCase);
    }

    internal static bool ValidIdentity(string? value, string prefix, int legacyHexLength)
    {
        if (value is null || !value.StartsWith(prefix, StringComparison.Ordinal)) return false;
        var tail = value[prefix.Length..];
        return (tail.Length == legacyHexLength && tail.All(Uri.IsHexDigit))
            || (tail.Length == 36 && Guid.TryParseExact(tail, "D", out _));
    }

    internal static void RejectCommandLineSecrets(IEnumerable<string> args)
    {
        foreach (var arg in args)
        {
            if (arg.Equals("--auto-logon-password", StringComparison.OrdinalIgnoreCase)
                || arg.StartsWith("--auto-logon-password=", StringComparison.OrdinalIgnoreCase))
                throw new InvalidOperationException("PLAINTEXT_ARGUMENT_REJECTED: use configure-autologon with the secure local password dialog, never a command-line password.");
        }
    }
}
