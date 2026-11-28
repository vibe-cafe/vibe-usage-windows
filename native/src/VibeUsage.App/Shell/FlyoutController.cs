using System;
using System.IO;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using VibeUsage.App.Panel;
using Windows.Graphics;

namespace VibeUsage.App.Shell;

/// <summary>
/// When the panel opens, where, and when it closes. Windows conventions, not
/// the Mac's: the panel sits against the taskbar next to the icon and closes
/// when the user goes elsewhere.
///
/// Opening (<see cref="OpenPolicy"/>): from a tray click the panel is shown
/// without activation (WS_EX_NOACTIVATE), so the window the user was in, or
/// the taskbar, keeps the keyboard and the input method's 中/英 does not flip;
/// there is no Esc then (no global hotkey, decision 4 of #85). Keyboard opens
/// and every start of the exe activate the panel: it has the keyboard and Esc.
///
/// Closing:
/// 1. An activated panel losing activation (<see cref="DismissSignal.Deactivated"/>).
/// 2. While open, an out-of-context EVENT_SYSTEM_FOREGROUND hook (accessibility
///    event, not an input hook) sees another program's window take the
///    foreground; removed as soon as the panel hides.
/// 3. After a tray open the taskbar already is the foreground, so a click on
///    an empty part of it changes nothing there. While open, two more
///    out-of-context hooks limited to the taskbar's process (Explorer) watch
///    mouse capture starting and focus moving inside the taskbar.
/// 4. The click that opened the panel can still land on the taskbar or the
///    「^」 overflow; <see cref="DismissPolicy"/> gives it a short grace.
/// 5. Click race on the icon: <see cref="TrayToggleGate"/>.
/// </summary>
internal sealed partial class FlyoutController : IDisposable
{
    private const double MarginDip = 12;

    private readonly PanelWindow panel;
    private readonly TrayIcon tray;
    private readonly LaunchOptions options;
    private readonly TrayToggleGate gate = new();
    private readonly Win32.WinEventProc foregroundProc; // must stay referenced: native code calls them
    private readonly Win32.WinEventProc shellProc;
    private readonly uint pid = (uint)Environment.ProcessId;
    private DispatcherQueueTimer? warmUpTimer;
    private IntPtr foregroundHook, captureHook, focusHook;
    private bool visible;
    private bool warmingUp;
    private bool relayoutQueued;
    private (Win32.POINT? Anchor, OpenSource Source)? pendingShow;
    private long shownAt;
    private uint shownAtTick;
    private Win32.POINT? anchor;
    private bool openedFromOverflow;
    private WindowFrame? frame;

    public Func<IntPtr>? SettingsWindowHandle { get; set; }

    public bool IsVisible => visible;

    /// <summary>A real open (warm-up excluded): reset the panel's navigation, tell the backend.</summary>
    public event Action<OpenSource>? Opening;

    public event Action? Hidden;

    private static long Now => Environment.TickCount64;

    public FlyoutController(PanelWindow panel, TrayIcon tray, LaunchOptions options)
    {
        this.panel = panel;
        this.tray = tray;
        this.options = options;
        foregroundProc = OnForegroundEvent;
        shellProc = OnShellEvent;

        panel.Activated += OnPanelActivated;
        panel.CloseRequested += reason => Hide(reason, auto: false);
        panel.ContentHeightChanged += OnContentHeightChanged;
        tray.Selected += OnTraySelected;
        tray.LeftButtonDown += () => gate.OnButtonDown(Now, visible);
    }

    /// <summary>
    /// Shows the window once while cloaked (invisible), so XAML builds and lays
    /// out the content before the first real open and the first click doesn't
    /// flash an empty frame. No backend reads here.
    ///
    /// With <paramref name="openAfterwards"/> (a start by the user) the cloaked
    /// window is shown activated: Windows lets a process the user just started
    /// take the foreground with its first window, and showing a window without
    /// activation first would use that up. Once laid out it is placed and
    /// uncloaked as the <see cref="OpenSource.Startup"/> open. With --autostart
    /// it is shown without activation and hidden again.
    /// </summary>
    public void WarmUp(bool openAfterwards)
    {
        warmingUp = true;
        Win32.SetDwmInt(panel.Hwnd, Win32.DWMWA_CLOAK, 1);
        panel.AppWindow.MoveAndResize(ComputeRect(null).Rect);
        panel.AppWindow.Show(openAfterwards);
        bool granted = openAfterwards && Win32.SetForegroundWindow(panel.Hwnd);
        warmUpTimer = panel.DispatcherQueue.CreateTimer();
        warmUpTimer.Interval = TimeSpan.FromMilliseconds(300);
        warmUpTimer.IsRepeating = false;
        warmUpTimer.Tick += (_, _) =>
        {
            warmUpTimer?.Stop();
            frame = panel.Frame() ?? frame;
            warmingUp = false;
            ShellLog.Write($"warm-up done frame={frame} contentDip={panel.MeasureDesiredHeightDip()} startupForeground={granted}");
            if (openAfterwards)
            {
                Show(null, OpenSource.Startup, alreadyShown: true);
            }
            else
            {
                panel.AppWindow.Hide();
                Win32.SetDwmInt(panel.Hwnd, Win32.DWMWA_CLOAK, 0);
            }
            if (pendingShow is { } p)
            {
                pendingShow = null;
                Show(p.Anchor, p.Source);
            }
        };
        warmUpTimer.Start();
    }

    // -- tray events ----------------------------------------------------------------------

    private void OnTraySelected(Win32.POINT at, bool keyboard)
    {
        var decision = gate.OnSelect(Now, visible, keyboard);
        ShellLog.Write($"tray select keyboard={keyboard} anchor={at} visible={visible} -> {decision}");
        switch (decision)
        {
            case TrayToggleGate.Decision.Open:
                Show(at, keyboard ? OpenSource.TrayKeyboard : OpenSource.TrayMouse);
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
        gate.Reset();
    }

    public TaskbarEdge EdgeAt(Win32.POINT at)
    {
        var mon = Win32.MonitorFromPoint(at, Win32.MONITOR_DEFAULTTONEAREST);
        if (!Win32.TryMonitorInfo(mon, out var mi)) return TaskbarEdge.Bottom;
        return PanelPlacement.EdgeOf(ToBox(mi.rcMonitor), ToBox(mi.rcWork), new Box(at.X - 8, at.Y - 8, 16, 16)).Edge;
    }

    // -- show / hide ----------------------------------------------------------------------

    /// <param name="alreadyShown">The startup warm-up already showed (cloaked) and activated the window.</param>
    public void Show(Win32.POINT? at, OpenSource source, bool alreadyShown = false)
    {
        if (warmingUp)
        {
            pendingShow = (at, source);
            return;
        }
        bool activate = OpenPolicy.Activates(source);
        if (visible)
        {
            if (activate) Activate(source);
            return;
        }

        anchor = at;
        openedFromOverflow = source == OpenSource.TrayMouse && IconInOverflow(at);
        Opening?.Invoke(source);
        panel.PrepareForOpen();
        var target = ComputeRect(at);
        ApplyRect(target.Rect);
        uint windowDpi = Win32.GetDpiForWindow(panel.Hwnd);
        if (windowDpi != target.Dpi)
        {
            // Moved onto a monitor with another scale: WinUI rescales the window
            // on WM_DPICHANGED; put back the size computed for this monitor.
            ShellLog.Write($"dpi changed on move {windowDpi} -> {target.Dpi}, placing again");
            target = ComputeRect(at);
            ApplyRect(target.Rect);
        }

        if (alreadyShown) Win32.SetDwmInt(panel.Hwnd, Win32.DWMWA_CLOAK, 0);
        else panel.AppWindow.Show(activate);
        bool granted = activate && (Win32.GetForegroundWindow() == panel.Hwnd || Win32.SetForegroundWindow(panel.Hwnd));
        visible = true;
        shownAt = Now;
        shownAtTick = unchecked((uint)Environment.TickCount);
        InstallHooks();
        panel.PlayEntrance(target.Edge);
        if (activate) panel.FocusFirst(OpenPolicy.ShowsFocusRing(source));

        var fg = Win32.GetForegroundWindow();
        var actual = Win32.VisibleRect(panel.Hwnd);
        ShellLog.Write($"show source={source} anchor={(at?.ToString() ?? "none")} overflow={openedFromOverflow} " +
                       $"edge={target.Edge} dpi={target.Dpi} contentDip={panel.MeasureDesiredHeightDip()} rect={actual} " +
                       $"activate={(activate ? granted.ToString() : "no")} isForeground={fg == panel.Hwnd} fg={Win32.ClassName(fg)}");
        WriteRectFile();

        // A mixed-DPI move can deliver WM_DPICHANGED after Show; check once more.
        panel.DispatcherQueue.TryEnqueue(DispatcherQueuePriority.Low, () =>
        {
            if (!visible) return;
            ApplyRect(ComputeRect(anchor).Rect);
            WriteRectFile();
        });
    }

    /// <summary>Already open and asked again by a start of the exe: give it the keyboard.</summary>
    private void Activate(OpenSource source)
    {
        bool granted = Win32.SetForegroundWindow(panel.Hwnd);
        panel.FocusFirst(OpenPolicy.ShowsFocusRing(source));
        ShellLog.Write($"show source={source} already open, activate={granted}");
    }

    public void Hide(string reason, bool auto)
    {
        if (!visible) return;
        visible = false;
        RemoveHooks();
        bool cursorOnIcon = auto && CursorOnIcon();
        panel.AppWindow.Hide();
        if (auto) gate.OnAutoClose(Now, cursorOnIcon);
        ShellLog.Write($"hide reason={reason} auto={auto} cursorOnIcon={cursorOnIcon} openFor={Now - shownAt}ms");
        Hidden?.Invoke();
    }

    // -- closing rules --------------------------------------------------------------------

    private void OnPanelActivated(object sender, WindowActivatedEventArgs e)
    {
        if (warmingUp) return;
        var state = e.WindowActivationState;
        ShellLog.Write($"activation {state} visible={visible}");
        if (state != WindowActivationState.Deactivated || !visible || options.Pin) return;
        // Look at the new foreground window once the switch has settled.
        panel.DispatcherQueue.TryEnqueue(DispatcherQueuePriority.Low, EvaluateDeactivation);
    }

    private void EvaluateDeactivation()
    {
        if (!visible) return;
        var fg = Win32.GetForegroundWindow();
        if (fg == panel.Hwnd) return;
        string cls = Win32.ClassName(fg);
        var reason = DismissPolicy.HideReason(DismissSignal.Deactivated, cls, Now - shownAt, IsOwnTransient(fg), openedFromOverflow);
        if (reason == null) ShellLog.Write($"deactivated to {cls} {Now - shownAt}ms after opening, staying open");
        else Hide(reason, auto: true);
    }

    private void OnForegroundEvent(IntPtr hook, uint eventType, IntPtr hwnd, int idObject, int idChild, uint thread, uint eventTime)
    {
        try
        {
            if (!visible || options.Pin || hwnd == panel.Hwnd) return;
            if (unchecked((int)(eventTime - shownAtTick)) < 0) return; // happened before this open
            string cls = Win32.ClassName(hwnd);
            var reason = DismissPolicy.HideReason(DismissSignal.ForegroundChanged, cls, Now - shownAt, false, openedFromOverflow);
            if (reason == null) ShellLog.Write($"foreground -> {cls} {Now - shownAt}ms after opening, staying open");
            else Hide(reason, auto: true);
        }
        catch (Exception ex)
        {
            ShellLog.Write("foreground hook error " + ex.Message);
        }
    }

    private void OnShellEvent(IntPtr hook, uint eventType, IntPtr hwnd, int idObject, int idChild, uint thread, uint eventTime)
    {
        try
        {
            if (!visible || options.Pin || hwnd == IntPtr.Zero) return;
            if (unchecked((int)(eventTime - shownAtTick)) < 0) return;
            string rootClass = Win32.ClassName(Win32.GetAncestor(hwnd, Win32.GA_ROOT));
            string kind = eventType == Win32.EVENT_SYSTEM_CAPTURESTART ? "capture" : "focus";
            var reason = DismissPolicy.HideReason(DismissSignal.TaskbarPressed, rootClass, Now - shownAt, false, openedFromOverflow);
            ShellLog.Write($"shell {kind} root={rootClass} window={Win32.ClassName(hwnd)} {Now - shownAt}ms -> {reason ?? "stay"}");
            if (reason != null) Hide(reason, auto: true);
        }
        catch (Exception ex)
        {
            ShellLog.Write("shell hook error " + ex.Message);
        }
    }

    /// <summary>Our own popups (list, menu, tooltip) may take activation; that isn't "clicking away".</summary>
    private bool IsOwnTransient(IntPtr hwnd)
    {
        if (hwnd == IntPtr.Zero || Win32.ProcessOf(hwnd) != pid) return false;
        return hwnd != tray.Hwnd && hwnd != (SettingsWindowHandle?.Invoke() ?? IntPtr.Zero);
    }

    private void InstallHooks()
    {
        if (foregroundHook == IntPtr.Zero)
        {
            foregroundHook = Win32.SetWinEventHook(Win32.EVENT_SYSTEM_FOREGROUND, Win32.EVENT_SYSTEM_FOREGROUND, IntPtr.Zero,
                foregroundProc, 0, 0, Win32.WINEVENT_OUTOFCONTEXT | Win32.WINEVENT_SKIPOWNPROCESS);
            if (foregroundHook == IntPtr.Zero) ShellLog.Write("foreground hook unavailable");
        }
        var taskbar = Win32.FindWindowW("Shell_TrayWnd", null);
        uint explorer = taskbar == IntPtr.Zero ? 0 : Win32.ProcessOf(taskbar);
        if (explorer == 0)
        {
            ShellLog.Write("taskbar not found, taskbar clicks not watched");
            return;
        }
        if (captureHook == IntPtr.Zero)
            captureHook = Win32.SetWinEventHook(Win32.EVENT_SYSTEM_CAPTURESTART, Win32.EVENT_SYSTEM_CAPTURESTART, IntPtr.Zero,
                shellProc, explorer, 0, Win32.WINEVENT_OUTOFCONTEXT);
        if (focusHook == IntPtr.Zero)
            focusHook = Win32.SetWinEventHook(Win32.EVENT_OBJECT_FOCUS, Win32.EVENT_OBJECT_FOCUS, IntPtr.Zero,
                shellProc, explorer, 0, Win32.WINEVENT_OUTOFCONTEXT);
        if (captureHook == IntPtr.Zero || focusHook == IntPtr.Zero)
            ShellLog.Write($"taskbar hooks capture={captureHook != IntPtr.Zero} focus={focusHook != IntPtr.Zero}");
    }

    private void RemoveHooks()
    {
        foreach (var hook in new[] { foregroundHook, captureHook, focusHook })
            if (hook != IntPtr.Zero) Win32.UnhookWinEvent(hook);
        foregroundHook = captureHook = focusHook = IntPtr.Zero;
    }

    private bool CursorOnIcon()
    {
        if (!Win32.GetCursorPos(out var p)) return false;
        if (tray.TryGetIconRect() is not Win32.RECT r) return false;
        return p.X >= r.Left - 2 && p.X < r.Right + 2 && p.Y >= r.Top - 2 && p.Y < r.Bottom + 2;
    }

    /// <summary>The click came from the 「^」 overflow when it is not on the taskbar itself.</summary>
    private static bool IconInOverflow(Win32.POINT? at)
    {
        if (at is not Win32.POINT a) return false;
        var taskbar = Win32.FindWindowW("Shell_TrayWnd", null);
        return taskbar != IntPtr.Zero && Win32.GetWindowRect(taskbar, out var r) && !r.Contains(a);
    }

    // -- placement --------------------------------------------------------------------------

    private void OnContentHeightChanged()
    {
        if (!visible || relayoutQueued) return;
        relayoutQueued = true;
        panel.DispatcherQueue.TryEnqueue(DispatcherQueuePriority.Low, () =>
        {
            relayoutQueued = false;
            if (!visible) return;
            ApplyRect(ComputeRect(anchor).Rect);
            WriteRectFile();
        });
    }

    private void ApplyRect(RectInt32 rect)
    {
        Win32.GetWindowRect(panel.Hwnd, out var now);
        if (now.Left == rect.X && now.Top == rect.Y && now.Width == rect.Width && now.Height == rect.Height) return;
        panel.AppWindow.MoveAndResize(rect);
    }

    private (RectInt32 Rect, TaskbarEdge Edge, uint Dpi) ComputeRect(Win32.POINT? at)
    {
        var iconRect = tray.TryGetIconRect();
        Box? icon = null;
        if (at is Win32.POINT a)
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
        // Placement is about the visible window: its client area gets the
        // panel's DIP size, and Windows' invisible resize margins (8 px left,
        // right and bottom on Windows 11) are added outside it.
        var live = panel.Frame();
        if (live is WindowFrame l) frame = l;
        var f = live ?? frame ?? new WindowFrame();
        double w = Math.Round(PanelWindow.WidthDip * scale) + f.VisibleExtraW;
        double desired = panel.MeasureDesiredHeightDip();
        if (desired < 100) desired = 600; // not laid out yet (warm-up)
        double h = PanelPlacement.Height(desired, scale, f.VisibleExtraH, work.H, margin);
        var p = PanelPlacement.Place(ToBox(mi.rcMonitor), work, icon, w, h, margin);
        return (new RectInt32((int)Math.Round(p.X) - f.Left, (int)Math.Round(p.Y) - f.Top,
            (int)w + f.Left + f.Right, (int)h + f.Top + f.Bottom), p.Edge, dpi);
    }

    private static Box ToBox(Win32.RECT r) => new(r.Left, r.Top, r.Width, r.Height);

    private void WriteRectFile()
    {
        if (string.IsNullOrEmpty(options.RectFile)) return;
        try
        {
            var r = Win32.VisibleRect(panel.Hwnd);
            File.WriteAllText(options.RectFile, $"{(long)panel.Hwnd} {r.Left} {r.Top} {r.Width} {r.Height}");
        }
        catch (Exception ex)
        {
            ShellLog.Write("rect file: " + ex.Message);
        }
    }

    public void Dispose()
    {
        warmUpTimer?.Stop();
        RemoveHooks();
    }
}
