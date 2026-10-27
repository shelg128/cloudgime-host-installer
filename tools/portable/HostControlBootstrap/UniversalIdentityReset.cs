using System.Text.Json;

namespace CloudgimeHostBootstrap;

// Fresh installation is an explicit, destructive-in-effect operator choice.
// Never reuse a stale activation identity; move only the known identity files
// to a local recovery archive so the active state is clean and auditable.
internal static class UniversalIdentityReset
{
    private static readonly string[] ActiveIdentityFiles =
    {
        Path.Combine("Host", "moonlight", "server", "host_activation_state.json"),
        Path.Combine("Host", "moonlight", "server", "host_provisioning_secret.xml"),
        Path.Combine("Host", "moonlight", "server", "pc_identity.json"),
        Path.Combine("moonlight", "server", "host_activation_state.json"),
        Path.Combine("moonlight", "server", "host_provisioning_secret.xml"),
        Path.Combine("moonlight", "server", "pc_identity.json"),
        Path.Combine("provisioning", "rental-reset-activation.json"),
        Path.Combine("provisioning", "host-activation-state.json"),
        Path.Combine("provisioning", "manifest.json"),
        Path.Combine("provisioning", "host_app.credential.dpapi"),
        Path.Combine("provisioning", "sentinel.credential.dpapi"),
        Path.Combine("provisioning", "machine-identity.dpapi"),
        Path.Combine("provisioning", "installer_renewal.credential.dpapi"),
        Path.Combine("provisioning", "installer-activation-intent.dpapi")
    };

    internal sealed record Receipt(
        string Status,
        string StateRoot,
        string ArchiveRoot,
        IReadOnlyList<string> ArchivedFiles,
        string ReceiptPath);

    internal static int Run(string? stateRootArgument)
    {
        try
        {
            var stateRoot = string.IsNullOrWhiteSpace(stateRootArgument)
                ? Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData), "CloudGime")
                : Path.GetFullPath(stateRootArgument);
            var receipt = Reset(stateRoot);
            Console.WriteLine(JsonSerializer.Serialize(new
            {
                ok = true,
                status = receipt.Status,
                stateRoot = receipt.StateRoot,
                archiveRoot = receipt.ArchiveRoot,
                archivedFileCount = receipt.ArchivedFiles.Count,
                receiptPath = receipt.ReceiptPath
            }));
            return 0;
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine($"UNIVERSAL_IDENTITY_RESET_FAILED: {ex.Message}");
            return 1;
        }
    }

    internal static Receipt Reset(string stateRoot)
    {
        var root = Path.GetFullPath(stateRoot);
        Directory.CreateDirectory(root);
        var archiveRoot = Path.Combine(
            root,
            "identity-archive",
            $"fresh-reset-{DateTime.UtcNow:yyyyMMddTHHmmssfffZ}-{Guid.NewGuid():N}");
        var receiptRoot = Path.Combine(root, "fresh-reset-receipts");
        var moved = new List<(string Source, string Destination, string Relative)>();

        try
        {
            foreach (var relative in ActiveIdentityFiles)
            {
                var source = ResolveWithinRoot(root, relative);
                if (!File.Exists(source)) continue;

                var destination = ResolveWithinRoot(archiveRoot, relative);
                Directory.CreateDirectory(Path.GetDirectoryName(destination)!);
                File.Move(source, destination);
                moved.Add((source, destination, relative));
            }

            Directory.CreateDirectory(receiptRoot);
            if (!Directory.Exists(receiptRoot))
                throw new IOException("UNIVERSAL_IDENTITY_RESET_RECEIPT_ROOT_UNAVAILABLE");
            var receiptPath = Path.Combine(
                receiptRoot,
                $"fresh-reset-{DateTime.UtcNow:yyyyMMddTHHmmssfffZ}-{Guid.NewGuid():N}.json");
            var receipt = new Receipt(
                "ARCHIVED_FOR_FRESH_INSTALL",
                root,
                archiveRoot,
                moved.Select(item => item.Relative).ToArray(),
                receiptPath);
            File.WriteAllText(receiptPath, JsonSerializer.Serialize(receipt));
            return receipt;
        }
        catch
        {
            foreach (var item in moved.AsEnumerable().Reverse())
            {
                try
                {
                    Directory.CreateDirectory(Path.GetDirectoryName(item.Source)!);
                    if (File.Exists(item.Destination)) File.Move(item.Destination, item.Source);
                }
                catch
                {
                    // Preserve the original failure; manual recovery remains possible
                    // from the archive path recorded by directory naming.
                }
            }
            throw;
        }
    }

    private static string ResolveWithinRoot(string root, string relative)
    {
        var fullRoot = Path.GetFullPath(root).TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar)
            + Path.DirectorySeparatorChar;
        var candidate = Path.GetFullPath(Path.Combine(root, relative));
        if (!candidate.StartsWith(fullRoot, StringComparison.OrdinalIgnoreCase))
            throw new InvalidDataException("UNIVERSAL_IDENTITY_RESET_PATH_ESCAPE");
        return candidate;
    }
}
