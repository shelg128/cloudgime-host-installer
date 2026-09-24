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
        => Inspect(journal, key, value => value.ValueKind == JsonValueKind.Object && value.EnumerateObject().Count() == 2
            && value.TryGetProperty("version", out var version) && version.TryGetInt32(out var v) && v == 1
            && value.TryGetProperty("phase", out var phase) && phase.ValueKind == JsonValueKind.String && phase.GetString() == "IDLE");

    internal static bool IsCleanupDone(ReadOnlySpan<byte> outbox, ReadOnlySpan<byte> key)
        => Inspect(outbox, key, value => value.ValueKind == JsonValueKind.Object
            && value.TryGetProperty("stage", out var stage) && stage.ValueKind == JsonValueKind.String && stage.GetString() == "DONE");

    private static bool Inspect(ReadOnlySpan<byte> encrypted, ReadOnlySpan<byte> key, Func<JsonElement,bool> accept)
    {
        if (encrypted.Length < 31 || encrypted.Length > 128 * 1024 || !encrypted[..3].SequenceEqual("v10"u8) || key.Length != 32)
            return false;
        var plain = new byte[encrypted.Length - 31];
        try
        {
            using var aes = new AesGcm(key, 16);
            aes.Decrypt(encrypted.Slice(3, 12), encrypted.Slice(15, plain.Length), encrypted[^16..], plain);
            using var document = JsonDocument.Parse(plain);
            return accept(document.RootElement);
        }
        catch (Exception e) when (e is CryptographicException or JsonException or InvalidOperationException) { return false; }
        finally { CryptographicOperations.ZeroMemory(plain); }
    }
}
