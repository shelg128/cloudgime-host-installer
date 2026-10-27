using System.Security.AccessControl;
using System.Security.Principal;
using System.Text;
using CloudgimeHostBootstrap;

internal static class InstallerProvisionPolicyTests
{
    internal static void Run()
    {
        int count = 0;
        void Check(bool ok) { count++; if (!ok) throw new Exception("PROVISION_POLICY_TEST_FAILED"); }
        void Reject(Action action, string expectedMessage)
        {
            count++;
            try { action(); }
            catch (InvalidDataException ex) when (ex.Message == expectedMessage) { return; }
            throw new Exception($"EXPECTED_REJECTION_{expectedMessage}");
        }

        var tempRoot = Path.Combine(Path.GetTempPath(), "cg-provision-test-" + Guid.NewGuid().ToString("N"));
        var secretsRoot = Path.Combine(tempRoot, "secrets");
        Directory.CreateDirectory(secretsRoot);

        try
        {
            // 1. Path validation tests
            var validFile = Path.Combine(secretsRoot, "setup-token-abcdef123456");
            InstallerProvisionPolicy.ValidateTokenFilePath(validFile, secretsRoot);
            Check(true);

            // Outside root
            Reject(() => InstallerProvisionPolicy.ValidateTokenFilePath(Path.Combine(tempRoot, "setup-token-12345678"), secretsRoot), "INSTALLER_PROVISION_TOKEN_FILE_INVALID");
            // Path traversal
            Reject(() => InstallerProvisionPolicy.ValidateTokenFilePath(Path.Combine(secretsRoot, "..", "setup-token-12345678"), secretsRoot), "INSTALLER_PROVISION_TOKEN_FILE_INVALID");
            // Bad prefix
            Reject(() => InstallerProvisionPolicy.ValidateTokenFilePath(Path.Combine(secretsRoot, "token-abcdef123456"), secretsRoot), "INSTALLER_PROVISION_TOKEN_FILE_INVALID");
            // Too short filename
            Reject(() => InstallerProvisionPolicy.ValidateTokenFilePath(Path.Combine(secretsRoot, "setup-token-1"), secretsRoot), "INSTALLER_PROVISION_TOKEN_FILE_INVALID");
            // Empty / whitespace
            Reject(() => InstallerProvisionPolicy.ValidateTokenFilePath("", secretsRoot), "INSTALLER_PROVISION_TOKEN_FILE_INVALID");
            Reject(() => InstallerProvisionPolicy.ValidateTokenFilePath(validFile, ""), "INSTALLER_PROVISION_TOKEN_FILE_INVALID");

            // 2. Content validation tests
            InstallerProvisionPolicy.ValidateTokenContent("123456789012");
            Check(true);
            InstallerProvisionPolicy.ValidateTokenContent(new string('a', 4096));
            Check(true);
            Reject(() => InstallerProvisionPolicy.ValidateTokenContent("12345678901"), "INSTALLER_PROVISION_TOKEN_REQUIRED");
            Reject(() => InstallerProvisionPolicy.ValidateTokenContent(""), "INSTALLER_PROVISION_TOKEN_REQUIRED");
            Reject(() => InstallerProvisionPolicy.ValidateTokenContent("   "), "INSTALLER_PROVISION_TOKEN_REQUIRED");
            Reject(() => InstallerProvisionPolicy.ValidateTokenContent(new string('a', 4097)), "INSTALLER_PROVISION_TOKEN_REQUIRED");
            Reject(() => InstallerProvisionPolicy.ValidateTokenContent("token\n12345678"), "INSTALLER_PROVISION_TOKEN_REQUIRED");
            Reject(() => InstallerProvisionPolicy.ValidateTokenContent("token\r12345678"), "INSTALLER_PROVISION_TOKEN_REQUIRED");
            Reject(() => InstallerProvisionPolicy.ValidateTokenContent("token\012345678"), "INSTALLER_PROVISION_TOKEN_REQUIRED");

            // 3. Filename length and bad name rejections
            var shortNamedFile = Path.Combine(secretsRoot, "setup-token-");
            Reject(() => InstallerProvisionPolicy.ValidateTokenFilePath(shortNamedFile, secretsRoot), "INSTALLER_PROVISION_TOKEN_FILE_INVALID");
            var badNamedFile = Path.Combine(secretsRoot, "not-a-token-file");
            Reject(() => InstallerProvisionPolicy.ValidateTokenFilePath(badNamedFile, secretsRoot), "INSTALLER_PROVISION_TOKEN_FILE_INVALID");

            Check(InstallerProvisionPolicy.DefaultSecretsRoot.EndsWith(Path.Combine("CloudGime", "installer-secrets"), StringComparison.OrdinalIgnoreCase));
            Console.WriteLine($"INSTALLER_PROVISION_POLICY_ASSERTIONS={count}");
        }
        finally
        {
            try { Directory.Delete(tempRoot, true); } catch { }
        }
    }
}
