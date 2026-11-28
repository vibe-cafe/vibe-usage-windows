using System;
using System.IO;

namespace VibeUsage.App.Shell;

/// <summary>
/// One line per shell decision (tray, open, close and why, activation), so a
/// tester follows the steps and the log explains what happened. Other
/// programs' windows appear by class name only, never by title; no input,
/// settings values or credentials are written.
/// Default: %LOCALAPPDATA%\ai.vibecafe.vibe-usage.windows\logs\shell.log,
/// rolled over to shell.old.log past 1 MB at start.
/// </summary>
internal static class ShellLog
{
    private const long MaxBytes = 1024 * 1024;
    private static readonly object Gate = new();
    private static string dir = Path.Combine(AppPaths.LocalData, "logs");

    public static string FilePath => Path.Combine(dir, "shell.log");

    public static void UseDirectory(string? directory)
    {
        if (!string.IsNullOrWhiteSpace(directory)) dir = directory;
        try
        {
            var file = new FileInfo(FilePath);
            if (file.Exists && file.Length > MaxBytes) file.MoveTo(Path.Combine(dir, "shell.old.log"), overwrite: true);
        }
        catch (Exception)
        {
            // Logging never takes the app down.
        }
    }

    public static void Write(string line)
    {
        try
        {
            lock (Gate)
            {
                Directory.CreateDirectory(dir);
                File.AppendAllText(FilePath, $"{DateTime.Now:yyyy-MM-dd HH:mm:ss.fff} [{Environment.ProcessId}] {line}{Environment.NewLine}");
            }
        }
        catch (Exception)
        {
            // Logging never takes the app down.
        }
    }
}

/// <summary>The app's folders; the same names the frozen Tauri build uses, so the native build takes over its data.</summary>
internal static class AppPaths
{
    public const string Identifier = "ai.vibecafe.vibe-usage.windows";

    public static string LocalData =>
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), Identifier);

    public static string RoamingConfig =>
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), Identifier);
}
