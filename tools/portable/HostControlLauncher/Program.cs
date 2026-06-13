using System.Diagnostics;
using System.Runtime.InteropServices;

namespace HostControlLauncher;

internal static class Program
{
    [STAThread]
    private static int Main(string[] args)
    {
        try
        {
            var root = AppContext.BaseDirectory.TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar);
            var appPath = Path.Combine(root, "cloudgime-host-control.exe");
            var bundlePath = Path.Combine(root, "bundle");

            if (!File.Exists(appPath))
            {
                ShowError($"cloudgime-host-control.exe tidak ditemukan:\n{appPath}");
                return 1;
            }

            if (!Directory.Exists(bundlePath))
            {
                ShowError($"Folder bundle tidak ditemukan:\n{bundlePath}");
                return 1;
            }

            var extraArgs = args.Length == 0
                ? string.Empty
                : " " + string.Join(" ", args.Select(QuoteArgument));

            Process.Start(new ProcessStartInfo
            {
                FileName = appPath,
                Arguments = $"--bundle-root {QuoteArgument(bundlePath)}{extraArgs}",
                WorkingDirectory = root,
                UseShellExecute = false,
                CreateNoWindow = true,
                WindowStyle = ProcessWindowStyle.Hidden,
            });

            return 0;
        }
        catch (Exception ex)
        {
            ShowError($"Gagal membuka Cloudgime Host Control:\n{ex.Message}");
            return 1;
        }
    }

    private static string QuoteArgument(string value)
    {
        if (string.IsNullOrEmpty(value))
        {
            return "\"\"";
        }

        return $"\"{value.Replace("\"", "\\\"")}\"";
    }

    private static void ShowError(string message)
    {
        _ = MessageBoxW(IntPtr.Zero, message, "Cloudgime Host Control", 0x00000010);
    }

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern int MessageBoxW(IntPtr hWnd, string text, string caption, uint type);
}
