using System;
using System.IO;

namespace VibeUsage.PanelPoc.Shell;

/// <summary>
/// One line per decision (show, activation result, focus changes, why the
/// panel closed), so a tester only follows the steps and the log explains
/// what happened. Window class names only, never window titles.
/// Default location: %LOCALAPPDATA%\VibeUsagePanelPoc\panel-poc.log.
/// </summary>
internal static class PocLog
{
    private static readonly object Gate = new();
    private static string _dir = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "VibeUsagePanelPoc");

    public static string FilePath => Path.Combine(_dir, "panel-poc.log");

    public static void UseDirectory(string? dir)
    {
        if (!string.IsNullOrWhiteSpace(dir)) _dir = dir;
    }

    public static void Write(string line)
    {
        try
        {
            lock (Gate)
            {
                Directory.CreateDirectory(_dir);
                File.AppendAllText(FilePath, $"{DateTime.Now:yyyy-MM-dd HH:mm:ss.fff} {line}{Environment.NewLine}");
            }
        }
        catch
        {
            // Logging must never take the sample down.
        }
    }
}

/// <summary>Command-line switches. The CI screenshot job uses them; testers don't need any.</summary>
internal sealed class PocOptions
{
    /// <summary>Open the panel right after start, as if the tray icon had been clicked.</summary>
    public bool OpenAtStart { get; private set; }

    /// <summary>Never auto-close (screenshots on a headless runner, where focus is unpredictable).</summary>
    public bool Pin { get; private set; }

    /// <summary>Start with 使用分布 expanded.</summary>
    public bool Expand { get; private set; }

    /// <summary>Start on a subscription tab: codex, claude, cursor, grok.</summary>
    public string? Tab { get; private set; }

    /// <summary>Write the panel's screen rectangle here after it opens (for cropping screenshots).</summary>
    public string? RectFile { get; private set; }

    public string? LogDir { get; private set; }

    /// <summary>CI click test: keep writing the tray icon's rectangle here for a few seconds.</summary>
    public string? IconFile { get; private set; }

    public static PocOptions Parse(string[] args)
    {
        var o = new PocOptions();
        foreach (var raw in args)
        {
            var a = raw.Trim();
            if (a == "--open") o.OpenAtStart = true;
            else if (a == "--pin") o.Pin = true;
            else if (a == "--expand") o.Expand = true;
            else if (a.StartsWith("--tab=", StringComparison.Ordinal)) o.Tab = a[6..];
            else if (a.StartsWith("--rect-file=", StringComparison.Ordinal)) o.RectFile = a[12..];
            else if (a.StartsWith("--log-dir=", StringComparison.Ordinal)) o.LogDir = a[10..];
            else if (a.StartsWith("--icon-file=", StringComparison.Ordinal)) o.IconFile = a[12..];
        }
        return o;
    }
}
