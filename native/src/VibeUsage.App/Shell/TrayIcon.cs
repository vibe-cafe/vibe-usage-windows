using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.InteropServices;
using Microsoft.Win32;

namespace VibeUsage.App.Shell;

/// <summary>
/// The notification-area icon, straight on Shell_NotifyIcon (no library).
///
/// It lives on a hidden top-level tool window rather than a message-only
/// window: message-only windows get no broadcasts, and these matter:
/// "TaskbarCreated" (Explorer restarted: add the icon again),
/// WM_SETTINGCHANGE "ImmersiveColorSet" (taskbar went light or dark: swap the
/// icon), WM_DPICHANGED / WM_DISPLAYCHANGE (scale changed: reload the icon at
/// the new size) and the end-of-session messages. The window is created on
/// the UI thread, so the WinUI message loop pumps it and every callback runs
/// on the UI thread.
///
/// NOTIFYICON_VERSION_4: a left click arrives as NIN_SELECT, Enter or Space
/// on the icon focused with Win+B as NIN_KEYSELECT, a right click or the menu
/// key as WM_CONTEXTMENU; wParam carries the icon's anchor point. A right
/// click is preceded by the icon's WM_RBUTTONDOWN/UP, the keyboard's menu
/// isn't: that tells the two apart.
/// </summary>
internal sealed partial class TrayIcon : IDisposable
{
    public const string HostClassName = "VibeUsage.Native.TrayHost";
    public const uint WM_TRAY = Win32.WM_APP + 1;
    /// <summary>Posted by a second start of the exe: open the panel (wParam 1: and give it the keyboard).</summary>
    public const uint WM_OPEN_REQUEST = Win32.WM_APP + 2;
    private const uint IconId = 1;
    private const nuint RetryTimer = 1;

    private readonly Win32.WndProc wndProc; // must stay referenced: native code calls it
    private readonly uint taskbarCreated;
    private IntPtr icon;
    private string iconKey = "";
    private bool added;
    private int failedAdds;
    private int simulatedFailures;
    private string tip = "Vibe Usage";
    private IntPtr menuPreviousForeground;
    private readonly MenuSourceTracker menuSource = new();

    public IntPtr Hwnd { get; }

    public event Action<Win32.POINT, bool>? Selected;   // anchor, keyboard
    public event Action<Win32.POINT, bool>? ContextMenuRequested; // anchor, keyboard
    public event Action? LeftButtonDown;
    public event Action<bool>? OpenRequested;           // activate
    public event Action? SessionEnding;

    /// <param name="simulatedFailures">CI only: report this many adds as failed without calling Shell_NotifyIcon.</param>
    public TrayIcon(int simulatedFailures = 0)
    {
        this.simulatedFailures = simulatedFailures;
        wndProc = WndProc;
        var instance = Win32.GetModuleHandleW(null);
        var wc = new Win32.WNDCLASSEXW
        {
            cbSize = (uint)Marshal.SizeOf<Win32.WNDCLASSEXW>(),
            lpfnWndProc = Marshal.GetFunctionPointerForDelegate(wndProc),
            hInstance = instance,
            lpszClassName = HostClassName,
        };
        if (Win32.RegisterClassExW(ref wc) == 0)
            ShellLog.Write($"tray: RegisterClassEx failed err={Marshal.GetLastWin32Error()}");

        Hwnd = Win32.CreateWindowExW(Win32.WS_EX_TOOLWINDOW, HostClassName, "Vibe Usage",
            Win32.WS_POPUP, 0, 0, 0, 0, IntPtr.Zero, IntPtr.Zero, instance, IntPtr.Zero);
        if (Hwnd == IntPtr.Zero)
            ShellLog.Write($"tray: CreateWindowEx failed err={Marshal.GetLastWin32Error()}");

        taskbarCreated = Win32.RegisterWindowMessageW("TaskbarCreated");
        DarkMenus.Enable(Hwnd);
        LoadIcon(force: true);
        Add();
    }

    public bool IsAdded => added;

    public void SetTooltip(string text)
    {
        tip = text.Length > 127 ? text[..127] : text;
        if (!added) return;
        var nid = NewData();
        nid.uFlags = Win32.NIF_TIP | Win32.NIF_SHOWTIP;
        Win32.Shell_NotifyIconW(Win32.NIM_MODIFY, ref nid);
    }

    /// <summary>The icon's rectangle (physical px), or null when Windows can't tell (e.g. hidden in a closed overflow).</summary>
    public Win32.RECT? TryGetIconRect()
    {
        if (!added) return null;
        var id = new Win32.NOTIFYICONIDENTIFIER
        {
            cbSize = (uint)Marshal.SizeOf<Win32.NOTIFYICONIDENTIFIER>(),
            hWnd = Hwnd,
            uID = IconId,
        };
        return Win32.Shell_NotifyIconGetRect(ref id, out var r) == 0 && r.Width > 0 && r.Height > 0 ? r : null;
    }

    // -- adding, with retry -----------------------------------------------------------------

    private void Add()
    {
        Win32.KillTimer(Hwnd, RetryTimer);
        var nid = NewData();
        nid.uFlags = Win32.NIF_MESSAGE | Win32.NIF_ICON | Win32.NIF_TIP | Win32.NIF_SHOWTIP;
        nid.uCallbackMessage = WM_TRAY;
        nid.hIcon = icon;
        bool ok;
        if (simulatedFailures > 0)
        {
            simulatedFailures--;
            ok = false;
        }
        else
        {
            ok = Win32.Shell_NotifyIconW(Win32.NIM_ADD, ref nid);
            // Explorer may still hold the icon from before (a TaskbarCreated race): update it instead.
            if (!ok) ok = Win32.Shell_NotifyIconW(Win32.NIM_MODIFY, ref nid);
        }
        added = ok;
        if (!ok)
        {
            failedAdds++;
            var delay = TrayRetry.Delay(failedAdds);
            Win32.SetTimer(Hwnd, RetryTimer, (uint)delay.TotalMilliseconds, IntPtr.Zero);
            ShellLog.Write($"tray: add failed (attempt {failedAdds}), retry in {delay.TotalMilliseconds:0} ms");
            return;
        }
        nid.uVersion = Win32.NOTIFYICON_VERSION_4;
        bool v4 = Win32.Shell_NotifyIconW(Win32.NIM_SETVERSION, ref nid);
        ShellLog.Write($"tray: added after {failedAdds} failed attempt(s), version4={v4}");
        failedAdds = 0;
    }

    // -- icon for theme and scale -------------------------------------------------------------

    /// <summary>
    /// White glyph on a dark taskbar, black on a light one, at the small-icon
    /// size for the taskbar's DPI. The taskbar's theme (SystemUsesLightTheme)
    /// is separate from the apps' theme.
    /// </summary>
    private bool LoadIcon(bool force)
    {
        bool lightTaskbar = Registry.GetValue(
            @"HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
            "SystemUsesLightTheme", 0) is int v && v != 0;
        uint dpi = TaskbarDpi();
        int size = Win32.GetSystemMetricsForDpi(Win32.SM_CXSMICON, dpi);
        string key = $"{(lightTaskbar ? "light" : "dark")}-{size}";
        if (!force && key == iconKey && icon != IntPtr.Zero) return false;
        string file = Path.Combine(AppContext.BaseDirectory, "Assets", lightTaskbar ? "TrayLight.ico" : "TrayDark.ico");
        var loaded = Win32.LoadImageW(IntPtr.Zero, file, Win32.IMAGE_ICON, size, size, Win32.LR_LOADFROMFILE);
        if (loaded == IntPtr.Zero)
        {
            ShellLog.Write($"tray: LoadImage failed err={Marshal.GetLastWin32Error()} file={Path.GetFileName(file)}");
            return false;
        }
        if (icon != IntPtr.Zero) Win32.DestroyIcon(icon);
        icon = loaded;
        iconKey = key;
        ShellLog.Write($"tray: icon {(lightTaskbar ? "light" : "dark")} taskbar, {size}px at dpi {dpi}");
        return true;
    }

    /// <summary>The notification area draws at the DPI of the taskbar's monitor (the primary).</summary>
    private uint TaskbarDpi()
    {
        var taskbar = Win32.FindWindowW("Shell_TrayWnd", null);
        var monitor = taskbar != IntPtr.Zero
            ? Win32.MonitorFromWindow(taskbar, Win32.MONITOR_DEFAULTTOPRIMARY)
            : Win32.MonitorFromPoint(new Win32.POINT(0, 0), Win32.MONITOR_DEFAULTTOPRIMARY);
        return Win32.DpiOfMonitor(monitor);
    }

    private void RefreshIcon(string why)
    {
        if (!LoadIcon(force: false) || !added) return;
        var nid = NewData();
        nid.uFlags = Win32.NIF_ICON;
        nid.hIcon = icon;
        bool ok = Win32.Shell_NotifyIconW(Win32.NIM_MODIFY, ref nid);
        ShellLog.Write($"tray: icon reloaded ({why}) ok={ok}");
    }

    // -- right-click menu ---------------------------------------------------------------------

    /// <summary>
    /// Native right-click menu. The documented rules for notification-icon
    /// menus: the owner must be the foreground window while the menu is up
    /// (or it won't close when you click away), and WM_NULL is posted after
    /// it closes. Clicking the icon gives us the right to take the foreground.
    /// </summary>
    public uint ShowMenu(Win32.POINT anchor, TaskbarEdge edge, IReadOnlyList<(uint Id, string? Text)> items, uint defaultId)
    {
        var menu = Win32.CreatePopupMenu();
        foreach (var (id, text) in items)
        {
            if (text is null) Win32.AppendMenuW(menu, Win32.MF_SEPARATOR, UIntPtr.Zero, null);
            else Win32.AppendMenuW(menu, Win32.MF_STRING, (UIntPtr)id, text);
        }
        Win32.SetMenuDefaultItem(menu, defaultId, 0);
        DarkMenus.Refresh();

        // Windows 11 opens tray menus against the taskbar's inner edge, at the
        // click. The anchor from WM_CONTEXTMENU sits inside the taskbar, so the
        // menu is aligned to the taskbar edge (the work area's edge, or the
        // icon's edge when the taskbar auto-hides), and the taskbar strip is
        // passed as the excluded rectangle so the menu never covers it. All in
        // physical pixels (per-monitor aware), the same space as the anchor,
        // the monitor info and TrackPopupMenuEx, so it holds at any scale.
        var (x, y, flags, exclude) = MenuPlacement(anchor, edge);
        menuPreviousForeground = Win32.GetForegroundWindow();
        bool fg = Win32.SetForegroundWindow(Hwnd);
        var tpm = new Win32.TPMPARAMS { cbSize = (uint)Marshal.SizeOf<Win32.TPMPARAMS>(), rcExclude = exclude };
        int cmd = Win32.TrackPopupMenuExWithParams(menu,
            Win32.TPM_RETURNCMD | Win32.TPM_RIGHTBUTTON | Win32.TPM_NONOTIFY | flags, x, y, Hwnd, ref tpm);
        Win32.PostMessageW(Hwnd, Win32.WM_NULL, IntPtr.Zero, IntPtr.Zero);
        Win32.DestroyMenu(menu);
        ShellLog.Write($"menu: owner foreground={fg} anchor={anchor} at=({x},{y}) edge={edge} exclude={exclude} command={cmd}");
        return (uint)Math.Max(cmd, 0);
    }

    /// <summary>
    /// After the menu, give the foreground back to the window that had it
    /// (the taskbar the user clicked, or their app), so the menu's required
    /// SetForegroundWindow on our host doesn't leave keyboard focus (and the
    /// input-method indicator) on this process.
    /// </summary>
    public void RestoreForegroundAfterMenu()
    {
        var prev = menuPreviousForeground;
        menuPreviousForeground = IntPtr.Zero;
        if (prev == IntPtr.Zero || prev == Hwnd) return;
        bool ok = Win32.SetForegroundWindow(prev);
        ShellLog.Write($"menu: foreground back to {Win32.ClassName(prev)} ok={ok}");
    }

    private (int X, int Y, uint Flags, Win32.RECT Exclude) MenuPlacement(Win32.POINT anchor, TaskbarEdge edge)
    {
        var mon = Win32.MonitorFromPoint(anchor, Win32.MONITOR_DEFAULTTONEAREST);
        if (!Win32.TryMonitorInfo(mon, out var mi))
            return (anchor.X, anchor.Y, Win32.TPM_BOTTOMALIGN | Win32.TPM_LEFTALIGN, default);
        var m = mi.rcMonitor;
        var w = mi.rcWork;
        var iconRect = TryGetIconRect();
        uint horizontal = Win32.GetSystemMetrics(Win32.SM_MENUDROPALIGNMENT) != 0 ? Win32.TPM_RIGHTALIGN : Win32.TPM_LEFTALIGN;
        switch (edge)
        {
            case TaskbarEdge.Top:
            {
                int y = iconRect is Win32.RECT r && r.Bottom > w.Top ? Math.Max(w.Top, r.Bottom) : w.Top;
                return (anchor.X, y, Win32.TPM_TOPALIGN | horizontal | Win32.TPM_VERTICAL,
                    new Win32.RECT { Left = m.Left, Top = m.Top, Right = m.Right, Bottom = y });
            }
            case TaskbarEdge.Left:
            {
                int x = iconRect is Win32.RECT r && r.Right > w.Left ? Math.Max(w.Left, r.Right) : w.Left;
                return (x, anchor.Y, Win32.TPM_LEFTALIGN | Win32.TPM_TOPALIGN | Win32.TPM_HORIZONTAL,
                    new Win32.RECT { Left = m.Left, Top = m.Top, Right = x, Bottom = m.Bottom });
            }
            case TaskbarEdge.Right:
            {
                int x = iconRect is Win32.RECT r && r.Left < w.Right ? Math.Min(w.Right, r.Left) : w.Right;
                return (x, anchor.Y, Win32.TPM_RIGHTALIGN | Win32.TPM_TOPALIGN | Win32.TPM_HORIZONTAL,
                    new Win32.RECT { Left = x, Top = m.Top, Right = m.Right, Bottom = m.Bottom });
            }
            default:
            {
                // Bottom taskbar. An icon in the overflow flyout or on an
                // auto-hidden taskbar sits above the work area's bottom: then
                // the menu goes above the icon.
                int y = iconRect is Win32.RECT r && r.Top < w.Bottom && r.Contains(anchor) ? r.Top : w.Bottom;
                return (anchor.X, y, Win32.TPM_BOTTOMALIGN | horizontal | Win32.TPM_VERTICAL,
                    new Win32.RECT { Left = m.Left, Top = y, Right = m.Right, Bottom = m.Bottom });
            }
        }
    }

    // -- messages -----------------------------------------------------------------------------

    private IntPtr WndProc(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam)
    {
        try
        {
            if (msg == WM_TRAY)
            {
                uint evt = (uint)((long)lParam & 0xFFFF);
                var anchor = new Win32.POINT(Win32.LoWord(wParam), Win32.HiWord(wParam));
                switch (evt)
                {
                    case Win32.WM_LBUTTONDOWN:
                        LeftButtonDown?.Invoke();
                        break;
                    case Win32.WM_RBUTTONDOWN:
                    case Win32.WM_RBUTTONUP:
                        menuSource.OnRightButton(Environment.TickCount64);
                        break;
                    case Win32.NIN_SELECT:
                        Selected?.Invoke(anchor, false);
                        break;
                    case Win32.NIN_KEYSELECT:
                        Selected?.Invoke(anchor, true);
                        break;
                    case Win32.WM_CONTEXTMENU:
                        bool keyboard = menuSource.TakeIsKeyboard(Environment.TickCount64);
                        ShellLog.Write($"tray menu requested keyboard={keyboard}");
                        try
                        {
                            ContextMenuRequested?.Invoke(anchor, keyboard);
                        }
                        finally
                        {
                            menuSource.Clear();
                        }
                        break;
                }
                return IntPtr.Zero;
            }

            switch (msg)
            {
                case WM_OPEN_REQUEST:
                    OpenRequested?.Invoke(wParam != IntPtr.Zero);
                    return IntPtr.Zero;
                case Win32.WM_TIMER when wParam == (nint)RetryTimer:
                    Win32.KillTimer(Hwnd, RetryTimer);
                    if (!added) Add();
                    return IntPtr.Zero;
                case Win32.WM_DPICHANGED:
                case Win32.WM_DISPLAYCHANGE:
                    RefreshIcon(msg == Win32.WM_DPICHANGED ? "dpi" : "display");
                    break;
                case Win32.WM_SETTINGCHANGE:
                    RefreshIcon(lParam != IntPtr.Zero && Marshal.PtrToStringUni(lParam) == "ImmersiveColorSet" ? "theme" : "settings");
                    break;
                case Win32.WM_QUERYENDSESSION:
                    return 1;
                case Win32.WM_ENDSESSION when wParam != IntPtr.Zero:
                    SessionEnding?.Invoke();
                    return IntPtr.Zero;
            }

            if (msg == taskbarCreated && taskbarCreated != 0)
            {
                ShellLog.Write("tray: TaskbarCreated (Explorer restarted), adding the icon again");
                added = false;
                LoadIcon(force: true);
                Add();
                return IntPtr.Zero;
            }
        }
        catch (Exception ex)
        {
            ShellLog.Write("tray: callback error " + ex);
        }
        return Win32.DefWindowProcW(hWnd, msg, wParam, lParam);
    }

    private Win32.NOTIFYICONDATAW NewData() => new()
    {
        cbSize = (uint)Marshal.SizeOf<Win32.NOTIFYICONDATAW>(),
        hWnd = Hwnd,
        uID = IconId,
        szTip = tip,
        szInfo = string.Empty,
        szInfoTitle = string.Empty,
    };

    public void Dispose()
    {
        Win32.KillTimer(Hwnd, RetryTimer);
        if (added)
        {
            var nid = NewData();
            Win32.Shell_NotifyIconW(Win32.NIM_DELETE, ref nid);
            added = false;
        }
        if (icon != IntPtr.Zero) Win32.DestroyIcon(icon);
        icon = IntPtr.Zero;
        if (Hwnd != IntPtr.Zero) Win32.DestroyWindow(Hwnd);
    }

    /// <summary>
    /// Classic Win32 menus are light-only unless the process opts in to dark
    /// mode through uxtheme's unnamed exports (ordinals 133/135/136). They are
    /// undocumented but used by many shipping apps; on any failure the menu
    /// stays light. With "AllowDark" the menu follows the apps theme.
    /// </summary>
    private static unsafe class DarkMenus
    {
        private static IntPtr flush;

        public static void Enable(IntPtr hwnd)
        {
            try
            {
                if (Environment.OSVersion.Version.Build < 18362) return;
                var ux = Win32.LoadLibraryW("uxtheme.dll");
                if (ux == IntPtr.Zero) return;
                var allowForWindow = Win32.GetProcAddressOrdinal(ux, 133);
                var setPreferredAppMode = Win32.GetProcAddressOrdinal(ux, 135);
                flush = Win32.GetProcAddressOrdinal(ux, 136);
                if (setPreferredAppMode != IntPtr.Zero)
                    ((delegate* unmanaged<int, int>)setPreferredAppMode)(1); // AllowDark
                if (allowForWindow != IntPtr.Zero && hwnd != IntPtr.Zero)
                    ((delegate* unmanaged<IntPtr, int, int>)allowForWindow)(hwnd, 1);
                Refresh();
            }
            catch (Exception ex)
            {
                ShellLog.Write("tray: dark menus unavailable " + ex.Message);
            }
        }

        public static void Refresh()
        {
            if (flush != IntPtr.Zero) ((delegate* unmanaged<void>)flush)();
        }
    }
}
