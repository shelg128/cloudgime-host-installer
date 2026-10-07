using System.Globalization;

namespace DisplayPrepareHelper;

/// <summary>Ordered log evidence for the H.264 NVENC preflight. Not proof of video delivery.</summary>
internal static class NvencLogEvidence
{
    internal const int PolicyVersion = 1;
    internal const int MaxCharacters = 160_000;

    internal static string ReadTail(string path)
    {
        using var stream = new FileStream(path, FileMode.Open, FileAccess.Read,
            FileShare.ReadWrite | FileShare.Delete);
        var length = stream.Length;
        var start = Math.Max(0, length - MaxCharacters);
        stream.Position = start;
        var bytes = new byte[(int)(length - start)];
        var count = 0;
        while (count < bytes.Length)
        {
            var n = stream.Read(bytes, count, bytes.Length - count);
            if (n == 0) break;
            count += n;
        }
        var text = System.Text.Encoding.UTF8.GetString(bytes, 0, count);
        if (start > 0)
        {
            var newline = text.IndexOf('\n');
            text = newline >= 0 ? text[(newline + 1)..] : string.Empty;
        }
        return text;
    }

    internal static bool TryTimestamp(string line, out DateTimeOffset timestamp)
    {
        timestamp = default;
        if (string.IsNullOrWhiteSpace(line) || line[0] != '[') return false;
        var end = line.IndexOf(']');
        if (end <= 1 || end > 40) return false;
        string[] formats = ["yyyy:MM:dd:HH:mm:ss", "yyyy-MM-dd HH:mm:ss", "yyyy-MM-dd HH:mm:ss.FFFFFFF"];
        if (!DateTime.TryParseExact(line[1..end], formats, CultureInfo.InvariantCulture,
                DateTimeStyles.AssumeLocal, out var date)) return false;
        timestamp = new DateTimeOffset(date);
        return true;
    }

    internal static string? LatestUnrecoveredFailure(string text)
    {
        // Caller bounds bytes; this also bounds standalone callers. A partial first
        // line cannot become a recovery certificate.
        if (text.Length > MaxCharacters)
        {
            text = text[^MaxCharacters..];
            var newline = text.IndexOf('\n');
            text = newline >= 0 ? text[(newline + 1)..] : string.Empty;
        }
        string? failure = null;
        bool runtimeFailure = false, startupAfterFailure = false, clockRewind = false;
        DateTimeOffset? previousTime = null, failureTime = null;
        foreach (var raw in text.Split('\n'))
        {
            var line = raw.TrimEnd('\r');
            var hasTime = TryTimestamp(line, out var time);
            if (hasTime)
            {
                if (previousTime is not null && time < previousTime.Value) clockRewind = true;
                previousTime = time;
            }
            if (Contains(line, "Sunshine version:"))
            {
                // A restart alone is never success. It only permits a subsequent
                // H.264 discovery to supersede a prior runtime/encode failure.
                if (failure is not null) startupAfterFailure = true;
            }
            bool operational = Contains(line, "encode wait timeout") ||
                Contains(line, "NVENC returned empty packet") || Contains(line, "Could not encode video packet");
            bool init = Contains(line, "Failed to create encoder D3D11 device") ||
                Contains(line, "Could not open codec [h264_nvenc]") || Contains(line, "Failed to create encoder [h264_nvenc]");
            if (operational || init)
            {
                failure = line; runtimeFailure |= operational; startupAfterFailure = false;
                failureTime = hasTime ? time : null;
                continue;
            }
            // Require the actual completed H264 discovery, not 'Trying', 'Creating',
            // UI readiness or another codec's success. Order AND trustworthy times.
            if (failure is not null && IsH264Discovery(line) && hasTime && failureTime is not null &&
                time >= failureTime.Value && !clockRewind && (!runtimeFailure || startupAfterFailure))
            {
                failure = null; failureTime = null; runtimeFailure = false; startupAfterFailure = false;
            }
        }
        return failure;
    }

    private static bool Contains(string s, string marker) => s.Contains(marker, StringComparison.OrdinalIgnoreCase);
    private static bool IsH264Discovery(string line)
    {
        if (!TryTimestamp(line, out _)) return false;
        var end = line.IndexOf(']');
        return line[(end + 1)..].Trim().Equals(": Info: Found H.264 encoder: h264_nvenc [nvenc]", StringComparison.OrdinalIgnoreCase);
    }
}
