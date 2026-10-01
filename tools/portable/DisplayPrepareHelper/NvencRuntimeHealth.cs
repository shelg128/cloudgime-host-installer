namespace DisplayPrepareHelper;

/// <summary>Interpret actual NVENC runtime failure markers, not arbitrary log text.</summary>
internal static class NvencRuntimeHealth
{
    private static readonly string[] FailureMarkers =
    [
        "encode wait timeout", "NVENC returned empty packet", "Could not encode video packet",
        "Failed to create encoder D3D11 device", "Could not open codec [h264_nvenc]",
        "Failed to create encoder [h264_nvenc]"
    ];

    public static string FailureLines(string text)
    {
        var matches = text.Split(['\r', '\n'], StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
            .Where(line => FailureMarkers.Any(marker => line.Contains(marker, StringComparison.OrdinalIgnoreCase)))
            .ToArray();
        return string.Join(" | ", matches);
    }
}
