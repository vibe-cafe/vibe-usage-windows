using System;
using System.Collections.Generic;
using System.IO;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Windows.Graphics;

namespace VibeUsage.PanelPoc.Shell;

internal enum ShowSource { Mouse, Keyboard, Menu, External, Startup }

/// <summary>
/// When the panel opens, where, and when it closes.
///
/// Why the Tauri attempt failed (交接笔记/Windows托盘弹窗-验证.md): Tauri only
/// learns about focus loss through the WebView, which only gets focus after
/// Windows makes the window foreground. When Windows declined, nothing ever
/// "lost focus" and the panel stuck on screen. Here:
///
/// 1. Foreground: the panel is shown and SetForegroundWindow is called inside
///    the tray click's own message (NIN_SELECT). Explorer hands the icon's
///    process the right to take the foreground for that click, which is the
///    legitimate path for tray apps. No Alt-key injection, no
///    AttachThreadInput. The result (granted or not) goes to the log.
/// 2. Closing, normal path: the native window's own deactivation
///    (Window.Activated → Deactivated), not a WebView focus event.
/// 3. Closing, fallback: while the panel is open, an out-of-context
///    EVENT_SYSTEM_FOREGROUND hook (event driven, no polling, not an input
///    hook) notices the foreground moving to another app. This closes the
///    panel even if Windows never activated it. The hook is removed as soon
///    as the panel hides.
/// 4. Click race: see <see cref="TrayToggleGate"/>.
/// 5. Grace: in the first 400 ms after opening, the foreground moving to the
///    taskbar or the overflow ("^") flyout is the tail of the very click that
///    opened the panel (the overflow closes itself), so it doesn't close it.
/// </summary>
internal sealed class FlyoutController : IDisposable
{
    private const double MarginDip = 12;
    private const long OpeningGraceMs = 400;

    private static readonly HashSet<string> ShellTrayClasses = new(StringComparer.Ordinal)
    {
        "Shell_TrayWnd",
        "Shell_SecondaryTrayWnd",
        "NotifyIconOverflowWindow",
        "TopLevelWindowForOverflowXamlIsland",
        "XamlExplorerHostIslandWindow",
    };

    private readonly PanelWindow _panel;
    private readonly TrayIcon _tray;
    private readonly PocOptions _options;
    private readonly TrayToggleGate _gate = new();
    private readonly Win32.WinEventProc _winEventProc; // must stay referenced: native code calls it
    private readonly uint _pid = (uint)Environment.ProcessId;
    private DispatcherQueueTimer? _warmUpTimer;
    private IntPtr _hook;
    private bool _visible;
    private bool _warmingUp;
    private bool _relayoutQueued;
    private (Win32.POINT? Anchor, ShowSource Source)? _pendingShow;
    private long _shownAt;
    private uint _shownAtTick;
    private Win32.POINT? _anchor;

    public Func<IntPtr>? SettingsWindowHandle { get; set; }

    public bool IsVisible => _visible;

    private static long Now => Environment.TickCount64;

    public FlyoutController(PanelWindow panel, TrayIcon tray, PocOptions options)
    {
        _panel = panel;
        _tray = tray;
        _options = options;
        _winEventProc = OnForegroundEvent;

        _panel.Activated += OnPanelActivated;
        _panel.CloseRequested += reason => Hide(reason, auto: false);
        _panel.ContentHeightChanged += OnContentHeightChanged;
        _tray.Selected += OnTraySelected;
        _tray.LeftButtonDown += () => _gate.OnButtonDown(Now, _visible);
    }

    /// <summary>
    /// Shows the window once while cloaked (invisible) so XAML builds and lays
    /// out the content before the first real open; otherwise the first click
    /// could flash an empty frame.
    /// </summary>
    public void WarmUp(Action? done)
    {
        _warmingUp = true;
        Win32.SetDwmInt(_panel.Hwnd, Win32.DWMWA_CLOAK, 1);
        _panel.AppWindow.MoveAndResize(ComputeRect(null).Rect);
        _panel.Activate();
        _warmUpTimer = _panel.DispatcherQueue.CreateTimer();
        _warmUpTimer.Interval = TimeSpan.FromMilliseconds(300);
        _warmUpTimer.IsRepeating = false;
        _warmUpTimer.Tick += (_, _) =>
        {
            _warmUpTimer?.Stop();
            _panel.AppWindow.Hide();
            Win32.SetDwmInt(_panel.Hwnd, Win32.DWMWA_CLOAK, 0);
            _warmingUp = false;
            PocLog.Write("warm-up done");
            done?.Invoke();
            if (_pendingShow is { } p)
            {
                _pendingShow = null;
                Show(p.Anchor, p.Source);
            }
        };
        _warmUpTimer.Start();
    }

    // -- tray events ----------------------------------------------------------------------

    private void OnTraySelected(Win32.POINT anchor, bool keyboard)
    {
        var decision = _gate.OnSelect(Now, _visible, keyboard);
        PocLog.Write($"tray select keyboard={keyboard} anchor={anchor} visible={_visible} -> {decision}");
        switch (decision)
        {
            case TrayToggleGate.Decision.Open:
                Show(anchor, keyboard ? ShowSource.Keyboard : ShowSource.Mouse);
                break;
            case TrayToggleGate.Decision.Close:
                Hide("tray-click", auto: false);
                break;
        }
    }

    /// <summary>Right-click: the panel closes and the menu opens, like system flyouts.</summary>
    public void PrepareForMenu()
    {
        Hide("context-menu", auto: false);
        _gate.Reset();
    }

    public TaskbarEdge EdgeAt(Win32.POINT anchor)
    {
        var mon = Win32.MonitorFromPoint(anchor, Win32.MONITOR_DEFAULTTONEAREST);
        if (!Win32.TryMonitorInfo(mon, out var mi)) return TaskbarEdge.Bottom;
        return PanelPlacement.EdgeOf(ToBox(mi.rcMonitor), ToBox(mi.rcWork), new Box(anchor.X - 8, anchor.Y - 8, 16, 16)).Edge;
    }

    // -- show / hide ----------------------------------------------------------------------

    public void Show(Win32.POINT? anchor, ShowSource source)
    {
        if (_warmingUp)
        {
            _pendingShow = (anchor, source);
            return;
        }
        if (_visible)
        {
            Win32.SetForegroundWindow(_panel.Hwnd);
            return;
        }

        _anchor = anchor;
        _panel.PrepareForOpen();
        var target = ComputeRect(anchor);
        ApplyRect(target.Rect);
        uint windowDpi = Win32.GetDpiForWindow(_panel.Hwnd);
        if (windowDpi != target.Dpi)
        {
            // Moved onto a monitor with another scale: WinUI rescales the window
            // on WM_DPICHANGED; put back the size computed for this monitor.
            PocLog.Write($"dpi changed on move {windowDpi} -> {target.Dpi}, placing again");
            target = ComputeRect(anchor);
            ApplyRect(target.Rect);
        }

        _panel.AppWindow.Show(true);
        bool granted = Win32.SetForegroundWindow(_panel.Hwnd);
        var fg = Win32.GetForegroundWindow();

        _visible = true;
        _shownAt = Now;
        _shownAtTick = unchecked((uint)Environment.TickCount);
        InstallForegroundHook();
        _panel.PlayEntrance(target.Edge);
        _panel.FocusForOpen(source == ShowSource.Keyboard);

        Win32.GetWindowRect(_panel.Hwnd, out var actual);
        PocLog.Write($"show source={source} anchor={(anchor?.ToString() ?? "none")} icon={(_tray.TryGetIconRect()?.ToString() ?? "none")} edge={target.Edge} dpi={target.Dpi} " +
                     $"rect={actual} SetForegroundWindow={granted} isForeground={fg == _panel.Hwnd} fg={Win32.ClassName(fg)}");
        WriteRectFile(actual);

        // A mixed-DPI move can deliver WM_DPICHANGED after Show; check once more.
        _panel.DispatcherQueue.TryEnqueue(DispatcherQueuePriority.Low, () =>
        {
            if (_visible) ApplyRect(target.Rect);
        });
    }

    public void Hide(string reason, bool auto)
    {
        if (!_visible) return;
        _visible = false;
        RemoveForegroundHook();
        bool cursorOnIcon = auto && CursorOnIcon();
        _panel.AppWindow.Hide();
        if (auto) _gate.OnAutoClose(Now, cursorOnIcon);
        PocLog.Write($"hide reason={reason} auto={auto} cursorOnIcon={cursorOnIcon} openFor={Now - _shownAt}ms");
    }

    // -- closing rules ----------------------------------------------------------------------

    private void OnPanelActivated(object sender, WindowActivatedEventArgs e)
    {
        if (_warmingUp) return;
        var state = e.WindowActivationState;
        PocLog.Write($"activation {state} visible={_visible}");
        if (state != WindowActivationState.Deactivated || !_visible || _options.Pin) return;
        // Look at the new foreground window once the switch has settled.
        _panel.DispatcherQueue.TryEnqueue(DispatcherQueuePriority.Low, EvaluateDeactivation);
    }

    private void EvaluateDeactivation()
    {
        if (!_visible) return;
        var fg = Win32.GetForegroundWindow();
        if (fg == _panel.Hwnd) return;
        string cls = Win32.ClassName(fg);
        if (IsOwnTransient(fg))
        {
            PocLog.Write($"deactivated to own popup {cls}, staying open");
            return;
        }
        if (Now - _shownAt < OpeningGraceMs && ShellTrayClasses.Contains(cls))
        {
            PocLog.Write($"deactivated to {cls} {Now - _shownAt}ms after opening, staying open");
            return;
        }
        Hide($"deactivated fg={cls}", auto: true);
    }

    private void OnForegroundEvent(IntPtr hook, uint eventType, IntPtr hwnd, int idObject, int idChild,
        uint thread, uint eventTime)
    {
        try
        {
            if (!_visible || _options.Pin || hwnd == _panel.Hwnd) return;
            if (unchecked((int)(eventTime - _shownAtTick)) < 0) return; // happened before this open
            string cls = Win32.ClassName(hwnd);
            if (Now - _shownAt < OpeningGraceMs && ShellTrayClasses.Contains(cls))
            {
                PocLog.Write($"foreground -> {cls} {Now - _shownAt}ms after opening, staying open");
                return;
            }
            Hide($"foreground-changed fg={cls}", auto: true);
        }
        catch (Exception ex)
        {
            PocLog.Write("foreground hook error " + ex.Message);
        }
    }

    /// <summary>Our own popups (combo box list, menu flyout) may take activation; that isn't "clicking away".</summary>
    private bool IsOwnTransient(IntPtr hwnd)
    {
        if (hwnd == IntPtr.Zero || Win32.ProcessOf(hwnd) != _pid) return false;
        return hwnd != _tray.Hwnd && hwnd != (SettingsWindowHandle?.Invoke() ?? IntPtr.Zero);
    }

    private void InstallForegroundHook()
    {
        if (_hook != IntPtr.Zero) return;
        _hook = Win32.SetWinEventHook(Win32.EVENT_SYSTEM_FOREGROUND, Win32.EVENT_SYSTEM_FOREGROUND, IntPtr.Zero,
            _winEventProc, 0, 0, Win32.WINEVENT_OUTOFCONTEXT | Win32.WINEVENT_SKIPOWNPROCESS);
        if (_hook == IntPtr.Zero) PocLog.Write("foreground hook unavailable");
    }

    private void RemoveForegroundHook()
    {
        if (_hook == IntPtr.Zero) return;
        Win32.UnhookWinEvent(_hook);
        _hook = IntPtr.Zero;
    }

    private bool CursorOnIcon()
    {
        if (!Win32.GetCursorPos(out var p)) return false;
        if (_tray.TryGetIconRect() is not Win32.RECT r) return false;
        return p.X >= r.Left - 2 && p.X < r.Right + 2 && p.Y >= r.Top - 2 && p.Y < r.Bottom + 2;
    }

    // -- placement --------------------------------------------------------------------------

    private void OnContentHeightChanged()
    {
        if (!_visible || _relayoutQueued) return;
        _relayoutQueued = true;
        _panel.DispatcherQueue.TryEnqueue(DispatcherQueuePriority.Low, () =>
        {
            _relayoutQueued = false;
            if (!_visible) return;
            var target = ComputeRect(_anchor);
            ApplyRect(target.Rect);
        });
    }

    private void ApplyRect(RectInt32 rect)
    {
        Win32.GetWindowRect(_panel.Hwnd, out var now);
        if (now.Left == rect.X && now.Top == rect.Y && now.Width == rect.Width && now.Height == rect.Height) return;
        _panel.AppWindow.MoveAndResize(rect);
    }

    private (RectInt32 Rect, TaskbarEdge Edge, uint Dpi) ComputeRect(Win32.POINT? anchor)
    {
        var iconRect = _tray.TryGetIconRect();
        Box? icon = null;
        if (anchor is Win32.POINT a)
            icon = iconRect is Win32.RECT r && r.Contains(a) ? ToBox(r) : new Box(a.X - 8, a.Y - 8, 16, 16);
        else if (iconRect is Win32.RECT r2)
            icon = ToBox(r2);

        IntPtr monitor = icon is Box b
            ? Win32.MonitorFromPoint(new Win32.POINT((int)b.Cx, (int)b.Cy), Win32.MONITOR_DEFAULTTONEAREST)
            : Win32.MonitorFromPoint(new Win32.POINT(0, 0), Win32.MONITOR_DEFAULTTOPRIMARY);
        if (!Win32.TryMonitorInfo(monitor, out var mi))
        {
            mi.rcMonitor = new Win32.RECT { Right = 1920, Bottom = 1080 };
            mi.rcWork = new Win32.RECT { Right = 1920, Bottom = 1032 };
        }

        uint dpi = Win32.DpiOfMonitor(monitor);
        double scale = dpi / 96.0;
        var work = ToBox(mi.rcWork);
        double margin = Math.Round(MarginDip * scale);
        double w = Math.Round(PanelWindow.PanelWidthDip * scale);
        double desired = _panel.MeasureDesiredHeightDip();
        if (desired < 100) desired = 600; // not laid out yet (warm-up)
        double h = Math.Min(Math.Ceiling(desired * scale), work.H - 2 * margin);
        var p = PanelPlacement.Place(ToBox(mi.rcMonitor), work, icon, w, h, margin);
        return (new RectInt32((int)Math.Round(p.X), (int)Math.Round(p.Y), (int)w, (int)h), p.Edge, dpi);
    }

    private static Box ToBox(Win32.RECT r) => new(r.Left, r.Top, r.Width, r.Height);

    private void WriteRectFile(Win32.RECT r)
    {
        if (string.IsNullOrEmpty(_options.RectFile)) return;
        try
        {
            File.WriteAllText(_options.RectFile, $"{r.Left} {r.Top} {r.Width} {r.Height}");
        }
        catch (Exception ex)
        {
            PocLog.Write("rect file: " + ex.Message);
        }
    }

    public void Dispose() => RemoveForegroundHook();
}
