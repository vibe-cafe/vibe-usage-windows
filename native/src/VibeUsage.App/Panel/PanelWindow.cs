using System;
using System.Collections.Generic;
using System.Numerics;
using System.Runtime.InteropServices;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Hosting;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using VibeUsage.App.Shell;
using VibeUsage.App.State;
using VirtualKey = Windows.System.VirtualKey;
using WinRT.Interop;

namespace VibeUsage.App.Panel;

/// <summary>
/// The window's frame in physical pixels: the invisible resize margins Windows
/// keeps around the visible window (left, top, right, bottom), and what the
/// visible window adds to the client area.
/// </summary>
public readonly record struct WindowFrame(int Left, int Top, int Right, int Bottom, int VisibleExtraW, int VisibleExtraH);

/// <summary>
/// The tray panel window: the Mac new panel's container (<c>ModernPanelView</c>):
/// header and 概览 row on top and the footer stay put, the cards between them
/// take their natural height and scroll when the screen is too short; 520 DIP
/// wide; dark only. Window behaviour (where, when it opens and closes) is in
/// <see cref="FlyoutController"/>; this class only draws and reports.
/// No title bar, not resizable, not in the taskbar or Alt+Tab, always on top
/// while shown, Windows 11 rounded corners and the Mac panel's faint edge.
/// </summary>
internal sealed partial class PanelWindow : Window
{
    public const double WidthDip = Theme.PanelWidth;

    private readonly AppModel model;
    private readonly LaunchOptions options;
    private readonly Grid root;
    private readonly Grid headerHost = new();
    private readonly Grid rowHost = new();
    private readonly Grid footerHost = new();
    private readonly ScrollViewer scroller;
    private readonly StackPanel cards;
    private readonly HashSet<string> notedThisOpen = [];
    private DispatcherQueueTimer? clock;
    private bool firstOpen = true;

    public IntPtr Hwnd { get; }

    /// <summary>Set before quitting so the Alt+F4 handling doesn't veto the real close.</summary>
    public bool AllowClose { get; set; }

    /// <summary>Esc (when the panel has the keyboard) or Alt+F4: close the panel, not the app.</summary>
    public event Action<string>? CloseRequested;
    public event Action? ContentHeightChanged;
    public event Action? SettingsRequested;
    public event Action? QuitRequested;

    public PanelWindow(AppModel model, LaunchOptions options)
    {
        this.model = model;
        this.options = options;
        Title = "Vibe Usage";

        root = new Grid
        {
            RequestedTheme = ElementTheme.Dark,
            Background = new SolidColorBrush(Theme.Panel),
            Language = "zh-CN",
        };
        root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        root.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        cards = new StackPanel { Padding = new Thickness(Theme.CardInset, 0, Theme.CardInset, 0), Spacing = Theme.CardSpacing };
        // The scroll bar overlays the cards, so they stay put whether or not the middle scrolls (Mac: scrollIndicators(.never)).
        scroller = new ScrollViewer
        {
            HorizontalScrollMode = ScrollMode.Disabled,
            HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled,
            VerticalScrollMode = ScrollMode.Auto,
            VerticalScrollBarVisibility = ScrollBarVisibility.Auto,
            Content = cards,
        };
        AutomationProperties.SetName(scroller, "Vibe Usage");
        Grid.SetRow(headerHost, 0);
        Grid.SetRow(rowHost, 1);
        Grid.SetRow(scroller, 2);
        Grid.SetRow(footerHost, 3);
        root.Children.Add(headerHost);
        root.Children.Add(rowHost);
        root.Children.Add(scroller);
        root.Children.Add(footerHost);
        Content = root;

        root.KeyDown += OnKeyDown;
        root.Loaded += (_, _) => ShellLog.Write("panel content loaded");
        foreach (var part in new FrameworkElement[] { cards, rowHost, footerHost, headerHost })
            part.SizeChanged += (_, _) => ContentHeightChanged?.Invoke();
        scroller.ViewChanged += (_, e) =>
        {
            if (!e.IsIntermediate && scroller.VerticalOffset > 0) Note("scroll", $"offset={scroller.VerticalOffset:0}");
        };

        Hwnd = WindowNative.GetWindowHandle(this);
        var presenter = OverlappedPresenter.Create();
        presenter.IsResizable = false;
        presenter.IsMaximizable = false;
        presenter.IsMinimizable = false;
        presenter.IsAlwaysOnTop = true;
        presenter.SetBorderAndTitleBar(true, false);
        AppWindow.SetPresenter(presenter);
        AppWindow.IsShownInSwitchers = false;
        // Clicking the panel doesn't take the foreground from the user's window
        // (programmatic activation for keyboard opens and starts still works).
        var exStyle = (long)Win32.GetWindowLongPtr(Hwnd, Win32.GWL_EXSTYLE);
        Win32.SetWindowLongPtr(Hwnd, Win32.GWL_EXSTYLE, (IntPtr)(exStyle | Win32.WS_EX_NOACTIVATE));
        AppWindow.SetIcon(System.IO.Path.Combine(AppContext.BaseDirectory, "Assets", "AppIcon.ico"));
        AppWindow.Closing += (_, e) =>
        {
            if (AllowClose) return;
            e.Cancel = true;
            CloseRequested?.Invoke("alt-f4");
        };
        Win32.SetDwmInt(Hwnd, Win32.DWMWA_WINDOW_CORNER_PREFERENCE, Win32.DWMWCP_ROUND);
        Win32.SetDwmInt(Hwnd, Win32.DWMWA_USE_IMMERSIVE_DARK_MODE, 1);
        Win32.SetDwmInt(Hwnd, Win32.DWMWA_BORDER_COLOR, Theme.BorderColorRef);

        model.Changed += Render;
        Render();
    }

    // -- window behaviour ---------------------------------------------------------------------

    /// <summary>The window's frame (see <see cref="WindowFrame"/>), or null while DWM can't tell (window not shown yet).</summary>
    public WindowFrame? Frame()
    {
        if (!Win32.GetWindowRect(Hwnd, out var w) || !Win32.GetClientRect(Hwnd, out var c)) return null;
        if (Win32.DwmGetWindowAttribute(Hwnd, Win32.DWMWA_EXTENDED_FRAME_BOUNDS, out var e, Marshal.SizeOf<Win32.RECT>()) != 0
            || e.Width <= 0 || !Win32.IsWindowVisible(Hwnd))
            return null;
        return new WindowFrame(e.Left - w.Left, e.Top - w.Top, w.Right - e.Right, w.Bottom - e.Bottom,
            Math.Max(0, e.Width - c.Width), Math.Max(0, e.Height - c.Height));
    }

    /// <summary>Natural height of the whole panel at the fixed width, in DIPs.</summary>
    public double MeasureDesiredHeightDip()
    {
        root.Measure(new Windows.Foundation.Size(WidthDip, double.PositiveInfinity));
        return Math.Ceiling(root.DesiredSize.Height);
    }

    /// <summary>
    /// Called on every real open, after the model reset the navigation. The
    /// CI switches (--test-expand, --test-scroll-end) apply to the first open only.
    /// </summary>
    public void PrepareForOpen()
    {
        notedThisOpen.Clear();
        focusKey = null;
        bool scrollEnd = false;
        if (firstOpen)
        {
            firstOpen = false;
            if (options.Expand) model.Panel.DetailsExpanded = true;
            scrollEnd = options.ScrollEnd;
        }
        Render();
        scroller.ChangeView(null, 0, null, true);
        if (scrollEnd)
            DispatcherQueue.TryEnqueue(DispatcherQueuePriority.Low, () => scroller.ChangeView(null, scroller.ScrollableHeight, null, true));
        StartClock();
    }

    /// <summary>The panel was hidden.</summary>
    public void OnHidden() => clock?.Stop();

    /// <summary>「N 分钟前同步」 moves on while the panel is up (Mac: TimelineView every 30 s).</summary>
    private void StartClock()
    {
        if (clock == null)
        {
            clock = DispatcherQueue.CreateTimer();
            clock.Interval = TimeSpan.FromSeconds(30);
            clock.Tick += (_, _) => RenderFooter();
        }
        clock.Start();
    }

    /// <summary>Windows 11 flyouts slide in from the taskbar and fade up; nothing moves when the system turns animations off.</summary>
    public void PlayEntrance(TaskbarEdge edge)
    {
        try
        {
            var visual = ElementCompositionPreview.GetElementVisual(root);
            if (!new Windows.UI.ViewManagement.UISettings().AnimationsEnabled)
            {
                visual.Opacity = 1;
                return;
            }
            ElementCompositionPreview.SetIsTranslationEnabled(root, true);
            var compositor = visual.Compositor;
            const float distance = 28f;
            var from = edge switch
            {
                TaskbarEdge.Top => new Vector3(0, -distance, 0),
                TaskbarEdge.Left => new Vector3(-distance, 0, 0),
                TaskbarEdge.Right => new Vector3(distance, 0, 0),
                _ => new Vector3(0, distance, 0),
            };
            var decelerate = compositor.CreateCubicBezierEasingFunction(new Vector2(0.1f, 0.9f), new Vector2(0.2f, 1f));
            var slide = compositor.CreateVector3KeyFrameAnimation();
            slide.InsertKeyFrame(0f, from);
            slide.InsertKeyFrame(1f, Vector3.Zero, decelerate);
            slide.Duration = TimeSpan.FromMilliseconds(300);
            var fade = compositor.CreateScalarKeyFrameAnimation();
            fade.InsertKeyFrame(0f, 0f);
            fade.InsertKeyFrame(1f, 1f, compositor.CreateLinearEasingFunction());
            fade.Duration = TimeSpan.FromMilliseconds(167);
            visual.StartAnimation("Translation", slide);
            visual.StartAnimation("Opacity", fade);
        }
        catch (Exception ex)
        {
            ShellLog.Write("entrance animation failed: " + ex.Message);
        }
    }

    /// <summary>
    /// The panel was activated (keyboard open, a start of the exe): put the
    /// keyboard on the first control so Tab and Esc work; the focus ring only
    /// for keyboard opens.
    /// </summary>
    public void FocusFirst(bool ring)
    {
        DispatcherQueue.TryEnqueue(DispatcherQueuePriority.Low, () =>
        {
            string? key = focusables.ContainsKey("overview") ? "overview" : focusables.ContainsKey("web") ? "web" : null;
            if (key == null) return;
            bool ok = focusables[key].Focus(ring ? FocusState.Keyboard : FocusState.Programmatic);
            ShellLog.Write($"focus {key} ring={ring} ok={ok}");
        });
    }

    private void OnKeyDown(object sender, KeyRoutedEventArgs e)
    {
        if (e.Key != VirtualKey.Escape) return;
        e.Handled = true;
        CloseRequested?.Invoke("esc");
    }

    /// <summary>Logs the first hover, tooltip and scroll of each open (the CI checks them on the non-activated panel).</summary>
    private void Note(string kind, string detail)
    {
        if (notedThisOpen.Add(kind)) ShellLog.Write($"panel {kind} {detail}");
    }
}
