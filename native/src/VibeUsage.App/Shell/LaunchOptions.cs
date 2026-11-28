using System;
using System.Collections.Generic;
using System.Globalization;

namespace VibeUsage.App.Shell;

/// <summary>Which made-up data the fake backend shows (CI screenshots pick one).</summary>
public enum DemoScenario
{
    /// <summary>Four subscriptions: the 概览 row and the overview.</summary>
    Subscriptions,
    /// <summary>One subscription: no 概览 row, the card opens on it.</summary>
    Single,
}

/// <summary>
/// Command line. <c>--autostart</c> is what the sign-in entry passes; the
/// <c>--test-*</c> switches exist for the CI screenshot and tray jobs only.
/// </summary>
public sealed class LaunchOptions
{
    /// <summary>Started by the sign-in entry: stay in the tray, and a late second start does nothing.</summary>
    public bool Autostart { get; private set; }

    /// <summary>Write the log here instead of the app's local data folder.</summary>
    public string? LogDir { get; private set; }

    /// <summary>Never close by itself (screenshots on a runner, where the foreground is unpredictable).</summary>
    public bool Pin { get; private set; }

    /// <summary>The first open starts with 展开详情.</summary>
    public bool Expand { get; private set; }

    /// <summary>The first open scrolls the middle to its end (the expanded panel is taller than the runner's screen).</summary>
    public bool ScrollEnd { get; private set; }

    public DemoScenario Demo { get; private set; } = DemoScenario.Subscriptions;

    /// <summary>After every open, write the panel's window handle and visible rectangle here.</summary>
    public string? RectFile { get; private set; }

    /// <summary>Keep writing the tray icon's rectangle here (only the icon's own process can ask Windows for it).</summary>
    public string? IconFile { get; private set; }

    /// <summary>Pretend the first N tray icon adds failed, to exercise the retry.</summary>
    public int TrayAddFailures { get; private set; }

    public static LaunchOptions Parse(IEnumerable<string> args)
    {
        var o = new LaunchOptions();
        foreach (var raw in args)
        {
            var a = raw.Trim();
            if (a == "--autostart") o.Autostart = true;
            else if (Value(a, "--test-log-dir=") is string dir) o.LogDir = dir;
            else if (a == "--test-pin") o.Pin = true;
            else if (a == "--test-expand") o.Expand = true;
            else if (a == "--test-scroll-end") o.ScrollEnd = true;
            else if (Value(a, "--test-demo=") is string demo) o.Demo = demo == "single" ? DemoScenario.Single : DemoScenario.Subscriptions;
            else if (Value(a, "--test-rect-file=") is string rect) o.RectFile = rect;
            else if (Value(a, "--test-icon-file=") is string icon) o.IconFile = icon;
            else if (Value(a, "--test-tray-add-failures=") is string n
                     && int.TryParse(n, NumberStyles.None, CultureInfo.InvariantCulture, out var failures))
                o.TrayAddFailures = failures;
        }
        return o;
    }

    private static string? Value(string arg, string prefix) =>
        arg.StartsWith(prefix, StringComparison.Ordinal) && arg.Length > prefix.Length ? arg[prefix.Length..] : null;
}
