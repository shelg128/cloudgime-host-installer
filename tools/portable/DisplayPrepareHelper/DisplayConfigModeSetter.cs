using System.Runtime.InteropServices;

namespace DisplayPrepareHelper;

/// <summary>Persist the selected managed-display source mode using the Windows CCD
/// database, not a transient DEVMODE override. Never selects by display index/GPU model.</summary>
internal static class DisplayConfigModeSetter
{
    internal const uint SourceMode = 1, InvalidModeIndex = 0xffffffff;
    private const uint ActivePaths = 2, UseSupplied = 0x20, Validate = 0x40,
        Apply = 0x80, SaveToDatabase = 0x200, AllowChanges = 0x400;

    [StructLayout(LayoutKind.Explicit, Size = 72)]
    internal struct DisplayPath
    {
        [FieldOffset(0)] internal long SourceAdapter;
        [FieldOffset(8)] internal uint SourceId;
        [FieldOffset(12)] internal uint SourceIndex;
        [FieldOffset(16)] internal uint SourceStatus;
        [FieldOffset(20)] internal long TargetAdapter;
        [FieldOffset(28)] internal uint TargetId;
        [FieldOffset(32)] internal uint TargetIndex;
        [FieldOffset(36)] internal uint Technology;
        [FieldOffset(40)] internal uint Rotation;
        [FieldOffset(44)] internal uint Scaling;
        [FieldOffset(48)] internal uint RefreshNumerator;
        [FieldOffset(52)] internal uint RefreshDenominator;
        [FieldOffset(56)] internal uint Scanline;
        [FieldOffset(60)] internal int Available;
        [FieldOffset(64)] internal uint TargetStatus;
        [FieldOffset(68)] internal uint Flags;
    }
    [StructLayout(LayoutKind.Explicit, Size = 64)]
    internal struct DisplayMode
    {
        [FieldOffset(0)] internal uint Type;
        [FieldOffset(4)] internal uint Id;
        [FieldOffset(8)] internal long Adapter;
        // Preserve the full native union, including target timing/desktop-image data.
        [FieldOffset(16)] internal ulong Data0;
        [FieldOffset(24)] internal ulong Data1;
        [FieldOffset(32)] internal ulong Data2;
        [FieldOffset(40)] internal ulong Data3;
        [FieldOffset(48)] internal ulong Data4;
        [FieldOffset(56)] internal ulong Data5;
        [FieldOffset(16)] internal uint Width;
        [FieldOffset(20)] internal uint Height;
        [FieldOffset(24)] internal uint PixelFormat;
        [FieldOffset(28)] internal int X;
        [FieldOffset(32)] internal int Y;
    }
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct SourceName
    {
        internal uint Type, Size;
        internal long Adapter;
        internal uint Id;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] internal string Name;
    }
    // CCD header contains a 4-byte-aligned LUID. Use an explicit packet because
    // the default CLR alignment of a long would incorrectly pad this header.
    [DllImport("user32.dll")] private static extern int DisplayConfigGetDeviceInfo(IntPtr packet);
    [DllImport("user32.dll")] private static extern int GetDisplayConfigBufferSizes(uint flags, out uint paths, out uint modes);
    [DllImport("user32.dll")] private static extern int QueryDisplayConfig(uint flags, ref uint pathCount,
        [Out] DisplayPath[] paths, ref uint modeCount, [Out] DisplayMode[] modes, IntPtr topology);
    [DllImport("user32.dll")] private static extern int SetDisplayConfig(uint pathCount,
        [In] DisplayPath[] paths, uint modeCount, [In] DisplayMode[] modes, uint flags);

    internal sealed record Snapshot(DisplayPath[] Paths, DisplayMode[] Modes);
    internal sealed record Plan(Snapshot Before, Snapshot Desired, int PathIndex, int Width, int Height, int Fps);

    private static Snapshot Read()
    {
        for (int retry = 0; retry < 4; retry++) {
            int code = GetDisplayConfigBufferSizes(ActivePaths, out uint pc, out uint mc);
            if (code != 0) throw new System.ComponentModel.Win32Exception(code);
            if (pc is 0 or > 64 || mc > 512) throw new InvalidDataException("CCD topology bounds.");
            var paths = new DisplayPath[pc]; var modes = new DisplayMode[mc];
            code = QueryDisplayConfig(ActivePaths, ref pc, paths, ref mc, modes, IntPtr.Zero);
            if (code == 122) continue;
            if (code != 0) throw new System.ComponentModel.Win32Exception(code);
            Array.Resize(ref paths, checked((int)pc)); Array.Resize(ref modes, checked((int)mc));
            return new Snapshot(paths, modes);
        }
        throw new IOException("CCD topology remained busy.");
    }
    private static string ReadName(DisplayPath path)
    {
        IntPtr packet = Marshal.AllocHGlobal(84);
        try {
            for (int i = 0; i < 84; i++) Marshal.WriteByte(packet, i, 0);
            Marshal.WriteInt32(packet, 0, 1); Marshal.WriteInt32(packet, 4, 84);
            Marshal.WriteInt64(packet, 8, path.SourceAdapter); Marshal.WriteInt32(packet, 16, unchecked((int)path.SourceId));
            int code = DisplayConfigGetDeviceInfo(packet);
            if (code != 0) throw new System.ComponentModel.Win32Exception(code);
            return Marshal.PtrToStringUni(IntPtr.Add(packet, 20), 32)!.TrimEnd('\0');
        } finally { Marshal.FreeHGlobal(packet); }
    }
    private static DisplayMode Source(Snapshot s, DisplayPath p)
    {
        if (p.SourceIndex >= s.Modes.Length) throw new InvalidDataException("CCD source missing.");
        var m = s.Modes[p.SourceIndex];
        if (m.Type != SourceMode || m.Adapter != p.SourceAdapter || m.Id != p.SourceId)
            throw new InvalidDataException("CCD source binding invalid.");
        return m;
    }
    internal static Plan MakePlan(Snapshot before, int index, int width, int height, int fps)
    {
        if (index < 0 || index >= before.Paths.Length || width < 180 || height < 180 ||
            Math.Max(width, height) > 7680 || Math.Min(width, height) > 4320 ||
            (width & 1) != 0 || (height & 1) != 0 || fps is < 30 or > 120)
            throw new InvalidDataException("Invalid CCD request.");
        var target = before.Paths[index];
        if (before.Paths.Count(p => p.SourceAdapter == target.SourceAdapter && p.SourceId == target.SourceId) != 1)
            throw new InvalidDataException("Cloned source has no exclusive display authority.");
        var current = Source(before, target);
        if (before.Paths.Where((_, i) => i != index).Any(p => p.SourceIndex == target.SourceIndex))
            throw new InvalidDataException("Aliased CCD source.");
        var paths = (DisplayPath[])before.Paths.Clone(); var modes = (DisplayMode[])before.Modes.Clone();
        current.Width = (uint)width; current.Height = (uint)height;
        modes[target.SourceIndex] = current;
        target.TargetIndex = InvalidModeIndex; // Windows chooses matching timing from registered modes.
        target.Rotation = 1; // native portrait surface, never landscape plus rotation metadata
        target.RefreshNumerator = (uint)fps; target.RefreshDenominator = 1;
        paths[index] = target;
        return new Plan(before, new Snapshot(paths, modes), index, width, height, fps);
    }
    private static bool SameEndpoint(DisplayPath a, DisplayPath b) =>
        a.SourceAdapter == b.SourceAdapter && a.SourceId == b.SourceId && a.TargetAdapter == b.TargetAdapter && a.TargetId == b.TargetId;
    internal static bool Matches(Plan plan, Snapshot actual)
    {
        if (plan.Before.Paths.Length != actual.Paths.Length) return false;
        for (int i = 0; i < plan.Before.Paths.Length; i++) {
            var old = plan.Before.Paths[i]; var matches = actual.Paths.Where(p => SameEndpoint(old, p)).ToArray();
            if (matches.Length != 1) return false;
            var p = matches[0]; var m = Source(actual, p); var prev = Source(plan.Before, old);
            if (m.X != prev.X || m.Y != prev.Y || m.PixelFormat != prev.PixelFormat) return false;
            if (i == plan.PathIndex) {
                if (m.Width != plan.Width || m.Height != plan.Height || p.Rotation != 1 ||
                    p.RefreshDenominator == 0 || Math.Abs((double)p.RefreshNumerator / p.RefreshDenominator - plan.Fps) > 0.6) return false;
            } else if (m.Width != prev.Width || m.Height != prev.Height || p.Rotation != old.Rotation ||
                p.RefreshNumerator != old.RefreshNumerator || p.RefreshDenominator != old.RefreshDenominator) return false;
        }
        return true;
    }
    internal static bool ApplyExact(string deviceName, int width, int height, int fps)
    {
        var before = Read();
        var candidates = before.Paths.Select((p, i) => (Name: ReadName(p), Index: i))
            .Where(p => string.Equals(p.Name, deviceName, StringComparison.OrdinalIgnoreCase)).ToArray();
        if (candidates.Length != 1) return false;
        var plan = MakePlan(before, candidates[0].Index, width, height, fps);
        if (Matches(plan, before)) return true; // persisted profile already selected; no Windows write
        uint flags = UseSupplied | AllowChanges;
        int code = SetDisplayConfig((uint)plan.Desired.Paths.Length, plan.Desired.Paths, (uint)plan.Desired.Modes.Length, plan.Desired.Modes, flags | Validate);
        if (code != 0) return false; // no mutation, no transient fallback reported as success
        bool accepted = false;
        try {
            code = SetDisplayConfig((uint)plan.Desired.Paths.Length, plan.Desired.Paths, (uint)plan.Desired.Modes.Length, plan.Desired.Modes, flags | Apply | SaveToDatabase);
            if (code != 0) return false;
            Thread.Sleep(120);
            accepted = Matches(plan, Read());
            return accepted;
        } finally {
            if (!accepted) {
                // Restore only while the original endpoint set still exists; never
                // overwrite a topology hotplugged by someone else during the operation.
                var now = Read();
                if (now.Paths.Length == before.Paths.Length && before.Paths.All(p => now.Paths.Any(n => SameEndpoint(p, n)))) {
                    int restored = SetDisplayConfig((uint)before.Paths.Length, before.Paths, (uint)before.Modes.Length, before.Modes, UseSupplied | Apply | SaveToDatabase);
                    if (restored != 0) throw new IOException("CCD rollback failed: " + restored);
                } else throw new IOException("CCD topology changed during apply.");
            }
        }
    }
}
