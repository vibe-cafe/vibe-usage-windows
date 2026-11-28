using System;
using System.Collections.Generic;
using System.Globalization;

namespace VibeUsage.App.Shell;

/// <summary>
/// Which made-up data the fake backend shows: a scene of the Mac panel
/// renderer, by its design id (<c>State/DemoScenes.cs</c>; CI screenshots
/// pick one per run). Unknown ids show the default scene.
/// </summary>
public sealed record DemoScenario(string Id)
{
    /// <summary>Four subscriptions: the 概览 row and the overview (Mac 131a).</summary>
    public static readonly DemoScenario Subscriptions = new("131a");

    /// <summary>One subscription: no 概览 row, the card opens on it (Mac 132g).</summary>
    public static readonly DemoScenario Single = new("132g");

    public static DemoScenario Parse(string value) => value switch
    {
        "single" => Single,
        "subscriptions" => Subscriptions,
        _ => new(value),
    };

    /// <summary>The scene id, as the shell log writes it (「backend: fake (131a)」).</summary>
    public override string ToString() => Id;
}

/// <summary>
/// Command line. <c>--autostart</c> is what the sign-in entry passes;
/// <c>--fake-backend</c> runs the shell on the in-memory fake backend instead
/// of the Rust DLL (development, screenshots, tray tests); the
/// <c>--test-*</c> switches exist for the CI jobs only.
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

    /// <summary>The in-memory fake backend with made-up data instead of the Rust DLL. Any --test-demo implies it.</summary>
    public bool FakeBackend { get; private set; }

    /// <summary>Keep the backend's settings and logs under this folder instead of the user's app data (CI).</summary>
    public string? DataDir { get; private set; }

    /// <summary>The backend's roaming config and local data folders: the user's, or two folders under --test-data-dir.</summary>
    public (string Roaming, string Local) DataFolders(string userRoaming, string userLocal) =>
        DataDir is string root ? (System.IO.Path.Combine(root, "roaming"), System.IO.Path.Combine(root, "local")) : (userRoaming, userLocal);

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
            else if (a == "--fake-backend") o.FakeBackend = true;
            else if (Value(a, "--test-demo=") is string demo)
            {
                o.Demo = DemoScenario.Parse(demo);
                o.FakeBackend = true;
            }
            else if (Value(a, "--test-data-dir=") is string data) o.DataDir = data;
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
