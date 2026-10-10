namespace DisplayPrepareHelper;

// Observe only; never fight another display owner by repeatedly applying modes.
internal static class DisplayModeStability
{
    internal static bool Verify(int width, int height, Func<(int Width, int Height)?> read,
        Action<int> pause, Func<long> elapsedMs, int durationMs = 6000)
    {
        if (width <= 0 || height <= 0 || durationMs < 0 || durationMs > 10000)
            throw new ArgumentOutOfRangeException();
        long start = elapsedMs();
        while (true) {
            var actual = read();
            if (actual == null || actual.Value.Width != width || actual.Value.Height != height) return false;
            if (elapsedMs() - start >= durationMs) return true;
            pause(250);
        }
    }
}
