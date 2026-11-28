using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.InteropServices;
using Microsoft.Win32;

namespace VibeUsage.PanelPoc.Shell;

/// <summary>
/// The notification-area icon, straight on Shell_NotifyIcon (no library).
///
/// It lives on a hidden top-level window rather than a message-only window:
/// message-only windows don't get broadcasts, and two broadcasts matter here:
/// "TaskbarCreated" (Explorer restarted: add the icon again) and
/// WM_SETTINGCHANGE "ImmersiveColorSet" (taskbar went light/dark: swap the
/// icon). The window is created on the UI thread, so the WinUI message loop
/// pumps it and every callback runs on the UI thread.
///
/// NOTIFYICON_VERSION_4: a left click arrives as NIN_SELECT, Enter/Space on
/// a keyboard-focused icon (Win+B) as NIN_KEYSELECT, a right click or the
/// menu key as WM_CONTEXTMENU; wParam carries the icon's anchor point.
/// </summary>
internal sealed class TrayIcon : IDisposable
{
    public const string HostClassName = "VibeUsagePanelPoc.TrayHost";
    public const uint WM_TRAY = Win32.WM_APP + 1;
    /// <summary>Posted by a second launch of the exe: open the panel.</summary>
    public const uint WM_OPEN_REQUEST = Win32.WM_APP + 2;
    private const uint IconId = 1;

    private readonly Win32.WndProc _wndProc; // must stay referenced: native code calls it
    private readonly uint _taskbarCreated;
    private IntPtr _icon;
    private bool _added;
    private string _tip = "Vibe Usage";

    public IntPtr Hwnd { get; }

    public event Action<Win32.POINT, bool>? Selected;   // anchor, keyboard
    public event Action<Win32.POINT>? ContextMenuRequested;
    public event Action? LeftButtonDown;
    public event Action? OpenRequested;
    public event Action? EscapePressed;

    private const int EscapeHotKeyId = 1;
    private bool _escapeRegistered;
    private IntPtr _menuPreviousForeground;

    public TrayIcon()
    {
        _wndProc = WndProc;
        var instance = Win32.GetModuleHandleW(null);
        var wc = new Win32.WNDCLASSEXW
        {
            cbSize = (uint)Marshal.SizeOf<Win32.WNDCLASSEXW>(),
            lpfnWndProc = Marshal.GetFunctionPointerForDelegate(_wndProc),
            hInstance = instance,
            lpszClassName = HostClassName,
        };
        if (Win32.RegisterClassExW(ref wc) == 0)
            PocLog.Write($"tray: RegisterClassEx failed err={Marshal.GetLastWin32Error()}");

        Hwnd = Win32.CreateWindowExW(Win32.WS_EX_TOOLWINDOW, HostClassName, "Vibe Usage tray host",
            Win32.WS_POPUP, 0, 0, 0, 0, IntPtr.Zero, IntPtr.Zero, instance, IntPtr.Zero);
        if (Hwnd == IntPtr.Zero)
            PocLog.Write($"tray: CreateWindowEx failed err={Marshal.GetLastWin32Error()}");

        _taskbarCreated = Win32.RegisterWindowMessageW("TaskbarCreated");
        DarkMenus.Enable(Hwnd);
        LoadIconForTheme();
        Add();
    }

    public void SetTooltip(string tip)
    {
        _tip = tip.Length > 127 ? tip[..127] : tip;
        if (!_added) return;
        var nid = NewData();
        nid.uFlags = Win32.NIF_TIP | Win32.NIF_SHOWTIP;
        nid.szTip = _tip;
        Win32.Shell_NotifyIconW(Win32.NIM_MODIFY, ref nid);
    }

    /// <summary>
    /// Esc while the panel is open without keyboard focus (opened by mouse):
    /// a hotkey held only while the panel is on screen.
    /// </summary>
    public void RegisterEscape()
    {
        if (_escapeRegistered) return;
        _escapeRegistered = Win32.RegisterHotKey(Hwnd, EscapeHotKeyId, Win32.MOD_NOREPEAT, Win32.VK_ESCAPE);
        if (!_escapeRegistered) PocLog.Write($"esc hotkey unavailable err={Marshal.GetLastWin32Error()}");
    }

    public void UnregisterEscape()
    {
        if (!_escapeRegistered) return;
        Win32.UnregisterHotKey(Hwnd, EscapeHotKeyId);
        _escapeRegistered = false;
    }

    /// <summary>
    /// After the menu, give the foreground back to the window that had it
    /// (the taskbar the user clicked, or their app), so the menu's required
    /// SetForegroundWindow on our host doesn't leave keyboard focus (and the
    /// input-method indicator) on this process.
    /// </summary>
    public void RestoreForegroundAfterMenu()
    {
        var prev = _menuPreviousForeground;
        _menuPreviousForeground = IntPtr.Zero;
        if (prev == IntPtr.Zero || prev == Hwnd) return;
        bool ok = Win32.SetForegroundWindow(prev);
        PocLog.Write($"menu: foreground back to {Win32.ClassName(prev)} ok={ok}");
    }

    /// <summary>The icon's rectangle (physical px), or null when Windows can't tell (e.g. hidden in a closed overflow).</summary>
    public Win32.RECT? TryGetIconRect()
    {
        var id = new Win32.NOTIFYICONIDENTIFIER
        {
            cbSize = (uint)Marshal.SizeOf<Win32.NOTIFYICONIDENTIFIER>(),
            hWnd = Hwnd,
            uID = IconId,
        };
        return Win32.Shell_NotifyIconGetRect(ref id, out var r) == 0 && r.Width > 0 && r.Height > 0 ? r : null;
    }

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
        _menuPreviousForeground = Win32.GetForegroundWindow();
        bool fg = Win32.SetForegroundWindow(Hwnd);
        var tpm = new Win32.TPMPARAMS { cbSize = (uint)Marshal.SizeOf<Win32.TPMPARAMS>(), rcExclude = exclude };
        int cmd = Win32.TrackPopupMenuExWithParams(menu,
            Win32.TPM_RETURNCMD | Win32.TPM_RIGHTBUTTON | Win32.TPM_NONOTIFY | flags, x, y, Hwnd, ref tpm);
        Win32.PostMessageW(Hwnd, Win32.WM_NULL, IntPtr.Zero, IntPtr.Zero);
        Win32.DestroyMenu(menu);
        PocLog.Write($"menu: owner foreground={fg} anchor={anchor} at=({x},{y}) edge={edge} exclude={exclude} command={cmd}");
        return (uint)Math.Max(cmd, 0);
    }

    private (int X, int Y, uint Flags, Win32.RECT Exclude) MenuPlacement(Win32.POINT anchor, TaskbarEdge edge)
    {
        var mon = Win32.MonitorFromPoint(anchor, Win32.MONITOR_DEFAULTTONEAREST);
        if (!Win32.TryMonitorInfo(mon, out var mi))
            return (anchor.X, anchor.Y, Win32.TPM_BOTTOMALIGN | Win32.TPM_LEFTALIGN, default);
        var m = mi.rcMonitor;
        var w = mi.rcWork;
        var icon = TryGetIconRect();
        uint horizontal = Win32.GetSystemMetrics(Win32.SM_MENUDROPALIGNMENT) != 0 ? Win32.TPM_RIGHTALIGN : Win32.TPM_LEFTALIGN;
        switch (edge)
        {
            case TaskbarEdge.Top:
            {
                int y = icon is Win32.RECT r && r.Bottom > w.Top ? Math.Max(w.Top, r.Bottom) : w.Top;
                return (anchor.X, y, Win32.TPM_TOPALIGN | horizontal | Win32.TPM_VERTICAL,
                    new Win32.RECT { Left = m.Left, Top = m.Top, Right = m.Right, Bottom = y });
            }
            case TaskbarEdge.Left:
            {
                int x = icon is Win32.RECT r && r.Right > w.Left ? Math.Max(w.Left, r.Right) : w.Left;
                return (x, anchor.Y, Win32.TPM_LEFTALIGN | Win32.TPM_TOPALIGN | Win32.TPM_HORIZONTAL,
                    new Win32.RECT { Left = m.Left, Top = m.Top, Right = x, Bottom = m.Bottom });
            }
            case TaskbarEdge.Right:
            {
                int x = icon is Win32.RECT r && r.Left < w.Right ? Math.Min(w.Right, r.Left) : w.Right;
                return (x, anchor.Y, Win32.TPM_RIGHTALIGN | Win32.TPM_TOPALIGN | Win32.TPM_HORIZONTAL,
                    new Win32.RECT { Left = x, Top = m.Top, Right = m.Right, Bottom = m.Bottom });
            }
            default:
            {
                // Bottom taskbar. An icon in the overflow flyout or on an
                // auto-hidden taskbar sits above the work area's bottom: then
                // the menu goes above the icon.
                int y = icon is Win32.RECT r && r.Top < w.Bottom && r.Contains(anchor) ? r.Top : w.Bottom;
                return (anchor.X, y, Win32.TPM_BOTTOMALIGN | horizontal | Win32.TPM_VERTICAL,
                    new Win32.RECT { Left = m.Left, Top = y, Right = m.Right, Bottom = m.Bottom });
            }
        }
    }

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
                    case Win32.NIN_SELECT:
                        Selected?.Invoke(anchor, false);
                        break;
                    case Win32.NIN_KEYSELECT:
                        Selected?.Invoke(anchor, true);
                        break;
                    case Win32.WM_CONTEXTMENU:
                        ContextMenuRequested?.Invoke(anchor);
                        break;
                }
                return IntPtr.Zero;
            }

            if (msg == Win32.WM_HOTKEY && (int)wParam == EscapeHotKeyId)
            {
                EscapePressed?.Invoke();
                return IntPtr.Zero;
            }

            if (msg == WM_OPEN_REQUEST)
            {
                OpenRequested?.Invoke();
                return IntPtr.Zero;
            }

            if (msg == _taskbarCreated && _taskbarCreated != 0)
            {
                PocLog.Write("tray: TaskbarCreated (Explorer restarted), adding the icon again");
                _added = false;
                LoadIconForTheme();
                Add();
                return IntPtr.Zero;
            }

            if (msg == Win32.WM_SETTINGCHANGE && lParam != IntPtr.Zero &&
                Marshal.PtrToStringUni(lParam) == "ImmersiveColorSet")
            {
                LoadIconForTheme();
                Modify();
            }
        }
        catch (Exception ex)
        {
            PocLog.Write("tray: callback error " + ex);
        }
        return Win32.DefWindowProcW(hWnd, msg, wParam, lParam);
    }

    private Win32.NOTIFYICONDATAW NewData() => new()
    {
        cbSize = (uint)Marshal.SizeOf<Win32.NOTIFYICONDATAW>(),
        hWnd = Hwnd,
        uID = IconId,
        szTip = _tip,
        szInfo = string.Empty,
        szInfoTitle = string.Empty,
    };

    private void Add()
    {
        var nid = NewData();
        nid.uFlags = Win32.NIF_MESSAGE | Win32.NIF_ICON | Win32.NIF_TIP | Win32.NIF_SHOWTIP;
        nid.uCallbackMessage = WM_TRAY;
        nid.hIcon = _icon;
        _added = Win32.Shell_NotifyIconW(Win32.NIM_ADD, ref nid);
        nid.uVersion = Win32.NOTIFYICON_VERSION_4;
        bool v4 = _added && Win32.Shell_NotifyIconW(Win32.NIM_SETVERSION, ref nid);
        PocLog.Write($"tray: add={_added} version4={v4}");
    }

    private void Modify()
    {
        if (!_added) return;
        var nid = NewData();
        nid.uFlags = Win32.NIF_ICON;
        nid.hIcon = _icon;
        Win32.Shell_NotifyIconW(Win32.NIM_MODIFY, ref nid);
    }

    /// <summary>
    /// White glyph on a dark taskbar, black glyph on a light one, at the small
    /// icon size for the current DPI. The taskbar's theme
    /// (SystemUsesLightTheme) is separate from the apps' theme.
    /// </summary>
    private void LoadIconForTheme()
    {
        bool lightTaskbar = Registry.GetValue(
            @"HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
            "SystemUsesLightTheme", 0) is int v && v != 0;
        string file = Path.Combine(AppContext.BaseDirectory, "Assets", lightTaskbar ? "TrayLight.ico" : "TrayDark.ico");
        uint dpi = Hwnd != IntPtr.Zero ? Win32.GetDpiForWindow(Hwnd) : 96;
        int size = Win32.GetSystemMetricsForDpi(Win32.SM_CXSMICON, dpi == 0 ? 96 : dpi);
        var icon = Win32.LoadImageW(IntPtr.Zero, file, Win32.IMAGE_ICON, size, size, Win32.LR_LOADFROMFILE);
        if (icon == IntPtr.Zero)
        {
            PocLog.Write($"tray: LoadImage failed err={Marshal.GetLastWin32Error()} file={Path.GetFileName(file)}");
            return;
        }
        if (_icon != IntPtr.Zero) Win32.DestroyIcon(_icon);
        _icon = icon;
        PocLog.Write($"tray: icon {(lightTaskbar ? "light" : "dark")} taskbar, {size}px");
    }

    public void Dispose()
    {
        if (_added)
        {
            var nid = NewData();
            Win32.Shell_NotifyIconW(Win32.NIM_DELETE, ref nid);
            _added = false;
        }
        if (_icon != IntPtr.Zero) Win32.DestroyIcon(_icon);
        _icon = IntPtr.Zero;
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
        private static IntPtr _flush;

        public static void Enable(IntPtr hwnd)
        {
            try
            {
                if (Environment.OSVersion.Version.Build < 18362) return;
                var ux = Win32.LoadLibraryW("uxtheme.dll");
                if (ux == IntPtr.Zero) return;
                var allowForWindow = Win32.GetProcAddressOrdinal(ux, (IntPtr)133);
                var setPreferredAppMode = Win32.GetProcAddressOrdinal(ux, (IntPtr)135);
                _flush = Win32.GetProcAddressOrdinal(ux, (IntPtr)136);
                if (setPreferredAppMode != IntPtr.Zero)
                    ((delegate* unmanaged<int, int>)setPreferredAppMode)(1); // AllowDark
                if (allowForWindow != IntPtr.Zero && hwnd != IntPtr.Zero)
                    ((delegate* unmanaged<IntPtr, int, int>)allowForWindow)(hwnd, 1);
                Refresh();
            }
            catch (Exception ex)
            {
                PocLog.Write("tray: dark menus unavailable " + ex.Message);
            }
        }

        public static void Refresh()
        {
            if (_flush != IntPtr.Zero) ((delegate* unmanaged<void>)_flush)();
        }
    }
}
