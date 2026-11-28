using System;
using System.Collections.Generic;

namespace VibeUsage.App.Shell;

/// <summary>How the panel was asked to open.</summary>
public enum OpenSource
{
    /// <summary>Left click on the tray icon, on the taskbar or in the 「^」 overflow.</summary>
    TrayMouse,
    /// <summary>Enter or Space on the tray icon after Win+B.</summary>
    TrayKeyboard,
    /// <summary>「打开面板」 in the tray icon's menu opened by a right click.</summary>
    Menu,
    /// <summary>「打开面板」 in the tray icon's menu opened from the keyboard (menu key or Shift+F10 after Win+B).</summary>
    MenuKeyboard,
    /// <summary>The exe started again while running: Start menu, shortcut, restart after an update.</summary>
    Relaunch,
    /// <summary>This process started without --autostart.</summary>
    Startup,
}

/// <summary>
/// Who gets the keyboard when the panel opens (#85, maintainer decision 4).
/// A mouse open from the tray leaves the foreground with the taskbar: the
/// panel never takes keyboard focus, so the taskbar's 中/英 indicator does not
/// flip, and Esc is not available (no global hotkey); a click elsewhere or on
/// the icon closes it. Every other way in activates the panel, so it has the
/// keyboard, Esc closes it and clicking back to the previous window closes it.
/// </summary>
public static class OpenPolicy
{
    public static bool Activates(OpenSource source) =>
        source is OpenSource.TrayKeyboard or OpenSource.MenuKeyboard or OpenSource.Relaunch or OpenSource.Startup;

    /// <summary>Only keyboard opens show the focus ring on the first control.</summary>
    public static bool ShowsFocusRing(OpenSource source) => source is OpenSource.TrayKeyboard or OpenSource.MenuKeyboard;

    /// <summary>
    /// After the tray menu, the foreground goes back to where it was, except
    /// when 「打开面板」 came from the keyboard: then the panel takes it.
    /// Menu commands: 0 dismissed, 1 打开面板, 2 立即同步, 3 设置, 4 退出.
    /// </summary>
    public static bool RestoresForegroundAfterMenu(uint command, bool keyboard) =>
        command is 0 or 2 || command == 1 && !keyboard;
}

/// <summary>What told the open panel that the user went elsewhere.</summary>
public enum DismissSignal
{
    /// <summary>The panel had been activated and lost it.</summary>
    Deactivated,
    /// <summary>Another program's window became the foreground.</summary>
    ForegroundChanged,
    /// <summary>
    /// The taskbar took a click while it already was the foreground (a tray
    /// open leaves it there), so no foreground change follows.
    /// </summary>
    TaskbarPressed,
    /// <summary>
    /// Windows refused the panel the foreground on a start, so the window the
    /// user was in stayed in front; a click back into it changes nothing there
    /// either, but starts a mouse capture in that program.
    /// </summary>
    ForegroundAppPressed,
}

/// <summary>Whether a signal closes the panel. Pure, so the timing rules are unit-tested.</summary>
public static class DismissPolicy
{
    /// <summary>The click that opened the panel can still land on the taskbar or the overflow this long after.</summary>
    public const long OpeningGraceMs = 400;

    /// <summary>
    /// The 「^」 overflow closes itself after the click on our icon in it; its
    /// close and the focus moving back to the taskbar take longer than a
    /// plain taskbar click.
    /// </summary>
    public const long OverflowOpeningGraceMs = 1000;

    private static readonly HashSet<string> ShellTrayClasses = new(StringComparer.Ordinal)
    {
        "Shell_TrayWnd",
        "Shell_SecondaryTrayWnd",
        "NotifyIconOverflowWindow",
        "TopLevelWindowForOverflowXamlIsland",
        "XamlExplorerHostIslandWindow",
    };

    private static readonly HashSet<string> TaskbarClasses = new(StringComparer.Ordinal)
    {
        "Shell_TrayWnd",
        "Shell_SecondaryTrayWnd",
    };

    public static bool IsTaskbarOrOverflow(string windowClass) => ShellTrayClasses.Contains(windowClass);

    public static bool IsTaskbar(string windowClass) => TaskbarClasses.Contains(windowClass);

    /// <summary>
    /// Why the panel closes, or null when it stays.
    /// </summary>
    /// <param name="windowClass">The new foreground window's class; for <see cref="DismissSignal.TaskbarPressed"/> the class of the pressed window's root.</param>
    /// <param name="sinceOpenMs">Time since the panel was shown.</param>
    /// <param name="ownTransient">One of our own popups (tooltip, list, menu) took activation.</param>
    /// <param name="openedFromOverflow">The icon was in the 「^」 overflow when it was clicked.</param>
    public static string? HideReason(DismissSignal signal, string windowClass, long sinceOpenMs, bool ownTransient,
        bool openedFromOverflow = false)
    {
        if (ownTransient) return null;
        long grace = openedFromOverflow ? OverflowOpeningGraceMs : OpeningGraceMs;
        if (sinceOpenMs < grace && IsTaskbarOrOverflow(windowClass)) return null;
        if (signal == DismissSignal.TaskbarPressed && !IsTaskbar(windowClass)) return null;
        string what = signal switch
        {
            DismissSignal.Deactivated => "deactivated",
            DismissSignal.ForegroundChanged => "foreground-changed",
            DismissSignal.TaskbarPressed => "taskbar-pressed",
            _ => "foreground-app-pressed",
        };
        return $"{what} fg={windowClass}";
    }
}

/// <summary>
/// When to try adding the tray icon again. At sign-in Explorer may not take
/// the icon yet; TaskbarCreated also triggers an add, the timer covers a
/// taskbar that was already up but busy.
/// </summary>
public static class TrayRetry
{
    public const int MaxDelayMs = 15000;

    /// <summary>Delay before retry number <paramref name="attempt"/> (1-based): 0.5, 1, 2, 4, 8, then every 15 s.</summary>
    public static TimeSpan Delay(int attempt)
    {
        int step = Math.Clamp(attempt - 1, 0, 5);
        return TimeSpan.FromMilliseconds(Math.Min(MaxDelayMs, 500 << step));
    }
}
