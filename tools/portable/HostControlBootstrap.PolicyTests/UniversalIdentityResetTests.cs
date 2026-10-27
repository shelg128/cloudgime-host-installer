namespace CloudgimeHostBootstrap.PolicyTests;

internal static class UniversalIdentityResetTests
{
    internal static void Run()
    {
        var count = 0;
        var root = Path.Combine(Path.GetTempPath(), "cloudgime-universal-identity-reset-" + Guid.NewGuid().ToString("N"));
        try
        {
            var active = Path.Combine(root, "Host", "moonlight", "server");
            Directory.CreateDirectory(active);
            var activation = Path.Combine(active, "host_activation_state.json");
            var secret = Path.Combine(active, "host_provisioning_secret.xml");
            File.WriteAllText(activation, "{\"activation\":\"stale\"}");
            File.WriteAllText(secret, "dpapi-protected-placeholder");
            var unrelated = Path.Combine(root, "Host", "moonlight", "server", "unrelated.json");
            File.WriteAllText(unrelated, "keep");

            var receipt = UniversalIdentityReset.Reset(root);
            Check(receipt.Status == "ARCHIVED_FOR_FRESH_INSTALL"); count++;
            Check(receipt.StateRoot == Path.GetFullPath(root)); count++;
            Check(File.Exists(receipt.ReceiptPath)); count++;
            Check(!File.Exists(activation) && !File.Exists(secret)); count++;
            Check(File.Exists(unrelated)); count++;
            Check(receipt.ArchivedFiles.Count == 2); count++;
            Check(File.Exists(Path.Combine(receipt.ArchiveRoot, "Host", "moonlight", "server", "host_activation_state.json"))); count++;
            Check(File.Exists(Path.Combine(receipt.ArchiveRoot, "Host", "moonlight", "server", "host_provisioning_secret.xml"))); count++;

            var second = UniversalIdentityReset.Reset(root);
            Check(second.ArchivedFiles.Count == 0); count++;
            Check(File.Exists(second.ReceiptPath)); count++;
        }
        finally
        {
            try { if (Directory.Exists(root)) Directory.Delete(root, true); } catch { }
        }
        Console.WriteLine($"UNIVERSAL_IDENTITY_RESET_ASSERTIONS={count}");
    }

    private static void Check(bool value)
    {
        if (!value) throw new InvalidOperationException("UNIVERSAL_IDENTITY_RESET_ASSERTION_FAILED");
    }
}
