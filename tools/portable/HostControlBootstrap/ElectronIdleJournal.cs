using System.Security.Cryptography;
using System.Text;
using System.Text.Json;

namespace CloudgimeHostBootstrap;

// Read-only compatibility reader for Chromium/Electron v10 AES-GCM storage on Windows.
// This is NOT authorization: callers must also verify administrator, pinned SID,
// stopped renter processes and empty local session state. Unknown formats fail closed.
internal static class ElectronIdleJournal
{
    internal static bool IsIdle(ReadOnlySpan<byte> journal, ReadOnlySpan<byte> key)
    {
        if (journal.Length < 31 || journal.Length > 64 * 1024 || !journal[..3].SequenceEqual("v10"u8) || key.Length != 32)
            return false;
        var plain = new byte[journal.Length - 31];
        try
        {
            using var aes = new AesGcm(key, 16);
            aes.Decrypt(journal.Slice(3, 12), journal.Slice(15, plain.Length), journal[^16..], plain);
            using var document = JsonDocument.Parse(plain);
            var value = document.RootElement;
            return value.ValueKind == JsonValueKind.Object && value.EnumerateObject().Count() == 2
                && value.TryGetProperty("version", out var version) && version.TryGetInt32(out var v) && v == 1
                && value.TryGetProperty("phase", out var phase) && phase.ValueKind == JsonValueKind.String && phase.GetString() == "IDLE";
        }
        catch (Exception e) when (e is CryptographicException or JsonException or InvalidOperationException) { return false; }
        finally { CryptographicOperations.ZeroMemory(plain); }
    }
}
