using System;
using System.Collections.Generic;
using System.Linq;
using System.Numerics;
using System.Threading.Tasks;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Hosting;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Markup;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Shapes;
using VibeUsage.PanelPoc.Data;
using VibeUsage.PanelPoc.Shell;
using VibeUsage.PanelPoc.Views;
using Windows.System;
using Windows.UI;
using WinRT.Interop;

namespace VibeUsage.PanelPoc;

/// <summary>
/// The tray panel, drawn like the macOS new panel (vibe-usage-app
/// Views/Modern): same parts, sizes, colours and type, built from the Mac
/// constants. Window behaviour (tray anchoring, closing, focus) stays in
/// <see cref="FlyoutController"/> and follows Windows conventions.
/// </summary>
public readonly record struct WindowFrame(int Left, int Top, int Right, int Bottom, int VisibleExtraW, int VisibleExtraH);

public sealed partial class PanelWindow : Window
{
    public const double PanelWidthDip = Mt.PanelWidth;

    private readonly PocOptions _options;
    private readonly IReadOnlyList<Subscription> _enabled;
    private readonly Dictionary<string, Button> _focusables = new();
    private string? _selected;
    private int _period;
    private bool _expanded;
    private readonly bool[] _byCost = new bool[4];
    private bool _syncing;
    private bool _refreshing;
    private bool _firstOpen = true;
    private string? _focusKey;
    private bool _focusByKeyboard;

    public IntPtr Hwnd { get; }

    /// <summary>Set before quitting so Alt+F4 handling doesn't veto the real close.</summary>
    public bool AllowClose { get; set; }

    public event Action<string>? CloseRequested;
    public event Action? SettingsRequested;
    public event Action? QuitRequested;
    public event Action? ContentHeightChanged;

    internal PanelWindow(PocOptions options)
    {
        InitializeComponent();
        _options = options;
        Hwnd = WindowNative.GetWindowHandle(this);

        var presenter = OverlappedPresenter.Create();
        presenter.IsResizable = false;
        presenter.IsMaximizable = false;
        presenter.IsMinimizable = false;
        presenter.IsAlwaysOnTop = true;
        presenter.SetBorderAndTitleBar(true, false);
        AppWindow.SetPresenter(presenter);
        AppWindow.IsShownInSwitchers = false;
        AppWindow.SetIcon(System.IO.Path.Combine(AppContext.BaseDirectory, "Assets", "AppIcon.ico"));
        AppWindow.Closing += (_, e) =>
        {
            if (AllowClose) return;
            e.Cancel = true;
            CloseRequested?.Invoke("alt-f4");
        };
        Win32.SetDwmInt(Hwnd, Win32.DWMWA_WINDOW_CORNER_PREFERENCE, Win32.DWMWCP_ROUND);
        Win32.SetDwmInt(Hwnd, Win32.DWMWA_USE_IMMERSIVE_DARK_MODE, 1);
        // The Mac panel's faint light edge (#383A3D) as the window border (Windows 11).
        Win32.SetDwmInt(Hwnd, Win32.DWMWA_BORDER_COLOR, 0x003D3A38);

        RootGrid.Background = new SolidColorBrush(Mt.Panel);
        // Keep the user's input method as it is while the panel has focus (see KeepInputMethod).
        RootGrid.GotFocus += (_, _) => KeepInputMethod("focus");
        Activated += (_, e) =>
        {
            if (e.WindowActivationState != WindowActivationState.Deactivated) KeepInputMethod("activated");
        };
        ContentStack.SizeChanged += (_, _) => ContentHeightChanged?.Invoke();

        _enabled = string.IsNullOrEmpty(options.Only)
            ? FakeData.Subscriptions
            : FakeData.Subscriptions.Where(s => s.Id == options.Only).DefaultIfEmpty(FakeData.Subscriptions[0]).ToList();
        ApplyStartState();
        Render();
        if (options.ScrollEnd)
            Scroller.SizeChanged += (_, _) => DispatcherQueue.TryEnqueue(() => Scroller.ChangeView(null, Scroller.ScrollableHeight, null, true));
    }

    // -- window behaviour ---------------------------------------------------------------------

    /// <summary>
    /// The window's frame, physical pixels: the invisible resize margins
    /// Windows keeps around the visible window (left, top, right, bottom),
    /// and what the visible window adds to the client area. Null when DWM
    /// can't tell (window not shown yet).
    /// </summary>
    public WindowFrame? Frame()
    {
        if (!Win32.GetWindowRect(Hwnd, out var w) || !Win32.GetClientRect(Hwnd, out var c)) return null;
        if (Win32.DwmGetWindowAttribute(Hwnd, Win32.DWMWA_EXTENDED_FRAME_BOUNDS, out var e, System.Runtime.InteropServices.Marshal.SizeOf<Win32.RECT>()) != 0
            || e.Width <= 0 || !Win32.IsWindowVisible(Hwnd))
            return null;
        return new WindowFrame(e.Left - w.Left, e.Top - w.Top, w.Right - e.Right, w.Bottom - e.Bottom,
            Math.Max(0, e.Width - c.Width), Math.Max(0, e.Height - c.Height));
    }

    /// <summary>The visible window on screen (DWM extended frame bounds).</summary>
    internal Win32.RECT VisibleRect()
    {
        if (Win32.DwmGetWindowAttribute(Hwnd, Win32.DWMWA_EXTENDED_FRAME_BOUNDS, out var e, System.Runtime.InteropServices.Marshal.SizeOf<Win32.RECT>()) == 0 && e.Width > 0)
            return e;
        Win32.GetWindowRect(Hwnd, out var w);
        return w;
    }

    /// <summary>Natural height of the whole panel at the fixed width, in DIPs.</summary>
    public double MeasureDesiredHeightDip()
    {
        RootGrid.Measure(new Windows.Foundation.Size(PanelWidthDip, double.PositiveInfinity));
        return Math.Ceiling(RootGrid.DesiredSize.Height);
    }

    /// <summary>
    /// Like the Mac (AppState.panelWillOpen): every open starts on 概览, cost
    /// and collapsed. The command-line state (CI screenshots) applies to the
    /// first open only.
    /// </summary>
    public void PrepareForOpen()
    {
        _focusKey = null;
        if (_firstOpen) _firstOpen = false;
        else
        {
            _selected = null;
            _expanded = false;
            Render();
        }
        Scroller.ChangeView(null, 0, null, true);
    }

    private void ApplyStartState()
    {
        _expanded = _options.Expand;
        if (_enabled.Count > 1 && _options.Tab is string tab && _enabled.Any(s => s.Id == tab)) _selected = tab;
    }

    /// <summary>Windows 11 flyouts slide in from the taskbar and fade up (Fluent decelerate curve).</summary>
    public void PlayEntrance(TaskbarEdge edge)
    {
        try
        {
            ElementCompositionPreview.SetIsTranslationEnabled(RootGrid, true);
            var visual = ElementCompositionPreview.GetElementVisual(RootGrid);
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
            PocLog.Write("entrance animation failed: " + ex.Message);
        }
    }

    public void FocusForOpen(bool keyboard)
    {
        if (keyboard && _focusables.TryGetValue("overview", out var first)) first.Focus(FocusState.Keyboard);
        else if (keyboard && _focusables.TryGetValue("web", out var web)) web.Focus(FocusState.Keyboard);
        else FocusSink.Focus(FocusState.Programmatic);
    }

    private void OnRootKeyDown(object sender, KeyRoutedEventArgs e)
    {
        if (e.Key == VirtualKey.Escape)
        {
            e.Handled = true;
            CloseRequested?.Invoke("esc");
        }
    }

    /// <summary>Pretend to sync (tray menu 「立即同步」 and the footer button).</summary>
    public async void StartFakeSync()
    {
        if (_syncing) return;
        _syncing = true;
        RenderFooter();
        await Task.Delay(1500);
        _syncing = false;
        RenderFooter();
    }

    // -- input method ---------------------------------------------------------------------------

    private string _imeLast = "";

    /// <summary>
    /// WinUI turns the input method off for its window while keyboard focus is
    /// on something that is not a text box. Opening the panel moves focus to
    /// it, so the taskbar's 中/英 indicator showed the IME as off (英) and
    /// flipped back (中) when the panel closed: every tray click "switched"
    /// it. The panel has no text input, so it gives its windows the thread's
    /// default input context again after each activation and focus change;
    /// the indicator then keeps showing the user's own state. The log records
    /// the IMM context and TSF focus before and after, for the real-machine check.
    /// </summary>
    public void KeepInputMethod(string when)
    {
        try
        {
            var focus = Win32.GetFocus();
            string before = ImeState(focus);
            bool ok = Win32.ImmAssociateContextEx(Hwnd, IntPtr.Zero, Win32.IACE_CHILDREN | Win32.IACE_DEFAULT);
            string after = ImeState(Win32.GetFocus());
            string line = $"ime {when}: focus={Win32.ClassName(focus)} before[{before}] associateDefault={ok} after[{after}]";
            if (line == _imeLast) return;
            _imeLast = line;
            PocLog.Write(line);
        }
        catch (Exception ex)
        {
            PocLog.Write("ime: " + ex.Message);
        }
    }

    private static string ImeState(IntPtr hwnd)
    {
        var himc = Win32.ImmGetContext(hwnd);
        bool has = himc != IntPtr.Zero;
        if (has) Win32.ImmReleaseContext(hwnd, himc);
        return $"imm={(has ? "context" : "none")} tsf={TsfFocus()}";
    }

    [System.Runtime.InteropServices.DllImport("msctf.dll")]
    private static extern int TF_GetThreadMgr(out IntPtr threadMgr);

    /// <summary>Whether TSF has a focused document on this thread (ITfThreadMgr::GetFocus, vtable slot 7).</summary>
    private static unsafe string TsfFocus()
    {
        if (TF_GetThreadMgr(out var tim) != 0 || tim == IntPtr.Zero) return "no-thread-mgr";
        try
        {
            var vtbl = *(IntPtr**)tim;
            var getFocus = (delegate* unmanaged[Stdcall]<IntPtr, IntPtr*, int>)vtbl[7];
            IntPtr doc = IntPtr.Zero;
            int hr = getFocus(tim, &doc);
            if (doc != IntPtr.Zero) System.Runtime.InteropServices.Marshal.Release(doc);
            return hr != 0 ? $"hr=0x{hr:X8}" : doc != IntPtr.Zero ? "doc" : "no-doc";
        }
        finally
        {
            System.Runtime.InteropServices.Marshal.Release(tim);
        }
    }

    // -- rendering helpers ----------------------------------------------------------------------

    private static readonly Dictionary<string, object> ResourceCache = new();

    internal static object Res(string key)
    {
        if (ResourceCache.TryGetValue(key, out var cached)) return cached;
        object? value = null;
        try { value = Application.Current.Resources[key]; } catch (Exception) { }
        if (value is null)
        {
            var probe = (ContentControl)XamlReader.Load(
                "<ContentControl xmlns=\"http://schemas.microsoft.com/winfx/2006/xaml/presentation\" Tag=\"{StaticResource " + key + "}\" />");
            value = probe.Tag;
        }
        ResourceCache[key] = value;
        return value;
    }

    /// <summary>
    /// A chrome-less button around drawn content: keyboard focus, Enter/Space
    /// and Narrator as a normal Windows button; the pointer brightens it the
    /// Mac way (<paramref name="hover"/>).
    /// </summary>
    private Button Bare(UIElement content, string key, string name, Action click, Action<bool>? hover = null, string? tooltip = null)
    {
        var b = new Button
        {
            Style = (Style)Res("BareButtonStyle"),
            Content = content,
            Tag = key,
            HorizontalAlignment = HorizontalAlignment.Left,
            VerticalAlignment = VerticalAlignment.Center,
        };
        AutomationProperties.SetName(b, name);
        if (tooltip != null) ToolTipService.SetToolTip(b, tooltip);
        b.Click += (_, _) =>
        {
            _focusKey = key;
            _focusByKeyboard = b.FocusState == FocusState.Keyboard;
            click();
        };
        if (hover != null)
        {
            b.PointerEntered += (_, _) => hover(true);
            b.PointerExited += (_, _) => hover(false);
            b.PointerCanceled += (_, _) => hover(false);
        }
        _focusables[key] = b;
        return b;
    }

    /// <summary>Brushes of text and icons that light to the primary colour under the pointer (ModernTextButtonStyle).</summary>
    private static Action<bool> Lights(Brush rest, params Action<Brush>[] setters) => on =>
    {
        foreach (var set in setters) set(on ? Mt.Text : rest);
    };

    private static Rectangle VRule(double height, double alpha = 0.14) => new()
    {
        Width = 1, Height = height, Fill = new SolidColorBrush(Mt.TextC(alpha)), VerticalAlignment = VerticalAlignment.Center,
    };

    private static UIElement Placed(UIElement e, double left, double top, double? width = null, double? height = null)
    {
        if (e is FrameworkElement fe)
        {
            fe.HorizontalAlignment = HorizontalAlignment.Left;
            fe.VerticalAlignment = VerticalAlignment.Top;
            fe.Margin = new Thickness(left, top, 0, 0);
            if (width is double w) fe.Width = w;
            if (height is double h) fe.Height = h;
        }
        return e;
    }

    private Subscription? Focused => _enabled.Count == 1 ? _enabled[0] : _enabled.FirstOrDefault(s => s.Id == _selected);

    /// <summary>A selected subscription narrows 近期消耗 and the details to its tool (only with two or more enabled).</summary>
    private double Share => _enabled.Count > 1 && Focused is Subscription s ? s.UsageShare : 1.0;

    private void Render()
    {
        _focusables.Clear();
        HeaderHost.Children.Clear();
        HeaderHost.Children.Add(Header());
        SubscriptionRowHost.Children.Clear();
        SubscriptionRowHost.Children.Add(_enabled.Count > 1 ? SubscriptionRow() : new Grid { Height = 4 });
        ContentStack.Children.Clear();
        ContentStack.Children.Add(QuotaCard());
        ContentStack.Children.Add(UsageCard());
        if (_expanded) ContentStack.Children.Add(DetailsGrid());
        RenderFooter();
        RestoreFocus();
    }

    private void RenderFooter()
    {
        FooterHost.Children.Clear();
        FooterHost.Children.Add(Footer());
        RestoreFocus();
    }

    private void RestoreFocus()
    {
        if (_focusKey is string key && _focusables.TryGetValue(key, out var b))
        {
            var state = _focusByKeyboard ? FocusState.Keyboard : FocusState.Programmatic;
            DispatcherQueue.TryEnqueue(() => b.Focus(state));
        }
    }

    // -- header (ModernHeaderBar) -----------------------------------------------------------------

    private UIElement Header()
    {
        var g = new Grid { Height = 40, Padding = new Thickness(Mt.TextInset, 12, Mt.TextInset, 8) };
        double baseline = Mt.BaselineIn(20, (13, Face.Text));
        g.Children.Add(Mt.AtBaseline(Mt.T("Vibe Usage", 13, Face.Text, Wt.SemiBold, Mt.Text, -0.13), baseline));

        var links = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 16, HorizontalAlignment = HorizontalAlignment.Right };
        links.Children.Add(WebLink("网页版", "web", baseline));
        links.Children.Add(WebLink("排行榜", "rank", baseline));
        var settings = Mt.T("设置", 13, Face.Text, Wt.Regular, Mt.Secondary);
        links.Children.Add(Bare(Sized(Mt.AtBaseline(settings, baseline), 20), "settings", "设置", () => SettingsRequested?.Invoke(),
            Lights(Mt.Secondary, b => settings.Foreground = b)));
        g.Children.Add(links);
        return g;
    }

    private static Grid Sized(UIElement child, double height)
    {
        var g = new Grid { Height = height, Background = Mt.Transparent };
        g.Children.Add(child);
        return g;
    }

    private Button WebLink(string title, string key, double baseline)
    {
        var text = Mt.T(title, 13, Face.Text, Wt.Regular, Mt.Secondary);
        var arrowBrush = new SolidColorBrush(Mt.TextC(0.62 * 0.8));
        var arrow = (Canvas)Icons.ArrowUpRight(arrowBrush);
        arrow.VerticalAlignment = VerticalAlignment.Top;
        arrow.Margin = new Thickness(3, 7, 0, 0);
        var row = new StackPanel { Orientation = Orientation.Horizontal };
        row.Children.Add(Mt.AtBaseline(text, baseline));
        row.Children.Add(arrow);
        return Bare(Sized(row, 20), key, title, () => PocLog.Write($"{key} clicked (sample: no browser)"),
            on =>
            {
                text.Foreground = on ? Mt.Text : Mt.Secondary;
                foreach (var p in arrow.Children.OfType<Path>()) p.Stroke = on ? new SolidColorBrush(Mt.TextC(0.8)) : arrowBrush;
            });
    }

    // -- 概览 row (ModernSubscriptionRow) ----------------------------------------------------------

    private UIElement SubscriptionRow()
    {
        bool overview = Focused == null;
        var g = new Grid { Height = 46, Padding = new Thickness(Mt.TextInset, 0, Mt.TextInset, 12) };
        var row = new StackPanel { Orientation = Orientation.Horizontal, VerticalAlignment = VerticalAlignment.Center, Height = 34 };

        var label = Mt.T("概览", 13, Face.Text, overview ? Wt.SemiBold : Wt.Medium, overview ? Mt.Text : Mt.Secondary);
        var overviewContent = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 7 };
        overviewContent.Children.Add(Gauges.Lamp(overview));
        overviewContent.Children.Add(Mt.AtBaseline(label, Mt.BaselineIn(34, (13, Face.Text))));
        row.Children.Add(Bare(Sized(overviewContent, 34), "overview", "概览", () => { _selected = null; Render(); }));

        var sep = VRule(16);
        sep.Margin = new Thickness(12, 0, 12, 0);
        row.Children.Add(sep);

        var buttons = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6, VerticalAlignment = VerticalAlignment.Center };
        foreach (var s in _enabled)
        {
            bool selected = Focused?.Id == s.Id;
            var holder = new Grid { Width = Gauges.ButtonSize, Height = Gauges.ButtonSize };
            holder.Children.Add(Gauges.RoundButtonFace(s, selected, false));
            string tip = $"{s.Name}{(s.Plan != null ? " " + s.Plan : "")} · {s.ButtonStatus}";
            buttons.Children.Add(Bare(holder, "sub-" + s.Id, tip, () => { _selected = s.Id; Render(); },
                on =>
                {
                    holder.Children.Clear();
                    holder.Children.Add(Gauges.RoundButtonFace(s, selected, on));
                }, tip));
        }
        row.Children.Add(buttons);
        g.Children.Add(row);

        var gear = Icons.Fluent(Icons.Gear, 12.5, Mt.Tertiary);
        var gearBox = new Grid { Width = 22, Height = 22, Background = Mt.Transparent };
        gearBox.Children.Add(gear);
        var gearButton = Bare(gearBox, "gear", "打开订阅配额设置", () => SettingsRequested?.Invoke(),
            Lights(Mt.Tertiary, b => gear.Foreground = b), "打开订阅配额设置");
        gearButton.HorizontalAlignment = HorizontalAlignment.Right;
        gearButton.Margin = new Thickness(0, 0, -4, 0);
        g.Children.Add(gearButton);
        return g;
    }

    // -- cards ------------------------------------------------------------------------------------

    /// <summary>ModernCard: 20 DIP title row (13 semibold, secondary) with an accessory at its right, the body 8 below.</summary>
    private static Grid CardFrame(string title, UIElement? accessory, UIElement body)
    {
        var stack = new StackPanel { Spacing = 8 };
        var titleRow = new Grid { Height = 20 };
        titleRow.Children.Add(Mt.AtBaseline(Mt.T(title, 13, Face.Text, Wt.SemiBold, Mt.Secondary), Mt.BaselineIn(20, (13, Face.Text))));
        if (accessory is FrameworkElement fe)
        {
            fe.HorizontalAlignment = HorizontalAlignment.Right;
            fe.VerticalAlignment = VerticalAlignment.Center;
            titleRow.Children.Add(fe);
        }
        stack.Children.Add(titleRow);
        stack.Children.Add(body);
        return Mt.Card(stack);
    }

    // -- 订阅配额 ----------------------------------------------------------------------------------

    private UIElement QuotaCard()
    {
        UIElement icon = _refreshing ? Icons.Spinner(13) : Icons.Fluent(Icons.TwoArrowsCircle, 11, Mt.Secondary);
        var box = new Grid { Width = 22, Height = 22, Background = Mt.Transparent };
        box.Children.Add(icon);
        var refresh = Bare(box, "refresh", "刷新额度", async () =>
        {
            if (_refreshing) return;
            _refreshing = true;
            Render();
            await Task.Delay(1200);
            _refreshing = false;
            Render();
        }, icon is FontIcon fi ? Lights(Mt.Secondary, b => fi.Foreground = b) : null, "刷新额度");
        refresh.Margin = new Thickness(0, 0, -4, 0);

        UIElement body = Focused is Subscription s ? QuotaDetail(s) : QuotaOverview();
        return CardFrame("订阅配额", refresh, body);
    }

    private UIElement QuotaOverview()
    {
        var rows = FakeData.OverviewRows(_enabled);
        int red = _enabled.Count(s => s.CommentTone == Tone.Red);
        int amber = _enabled.Count(s => s.CommentTone == Tone.Amber);
        int attention = red + amber;
        Tone tone = red > 0 ? Tone.Red : amber > 0 ? Tone.Amber : Tone.Neutral;

        var g = new Grid { Height = Mt.CardBodyHeight };
        // Left: the count, its caption, 快用完 / 偏快 at the bottom.
        var big = Mt.Block(
            Mt.R(attention.ToString(), 44, Face.Mono, Wt.Light, Gauges.Toned(tone, Mt.Secondary), -2.64),
            Mt.Gap(2),
            Mt.R("/" + _enabled.Count, 22, Face.Mono, Wt.Light, Mt.Secondary, -0.88));
        g.Children.Add(Placed(Mt.AtBaseline(big, Mt.BaselineIn(44, (44, Face.Mono), (22, Face.Mono))), -2, 0));
        g.Children.Add(Placed(Mt.AtBaseline(Mt.T("个订阅要留意", 11, Face.Text, Wt.Regular, Mt.Secondary), 44 + Mt.BaselineIn(18, (11, Face.Text))), 0, 0));
        double countBaseline = Mt.BaselineIn(18, (11, Face.Text), (13, Face.Mono));
        AddCount(g, "快用完", red, Tone.Red, Mt.CardBodyHeight - 36 + countBaseline);
        AddCount(g, "偏快", amber, Tone.Amber, Mt.CardBodyHeight - 18 + countBaseline);

        // Right: one row per subscription.
        double pitch = rows.Count >= 4 ? 38.5 : 42.5;
        for (int i = 0; i < rows.Count; i++)
            g.Children.Add(Placed(OverviewRow(rows[i], pitch), Mt.LeadingColumnWidth, i * pitch, Mt.RightColumnWidth, pitch));
        return g;
    }

    private static void AddCount(Grid g, string label, int value, Tone tone, double baseline)
    {
        g.Children.Add(Placed(Mt.AtBaseline(Mt.T(label, 11, Face.Text, Wt.Regular, Mt.Secondary), baseline), 0, 0));
        g.Children.Add(Placed(Mt.AtBaseline(Mt.T(value.ToString(), 13, Face.Mono, Wt.Medium, value > 0 ? Gauges.Toned(tone, Mt.Secondary) : Mt.Secondary), baseline), 46, 0));
    }

    /// <summary>ModernOverviewRow: mark, name, the ring windows' percentages; the bar; the comment at the right.</summary>
    private Button OverviewRow(Subscription s, double pitch)
    {
        var g = new Grid { Width = Mt.RightColumnWidth, Height = pitch, Background = Mt.Transparent };
        var band = new Border
        {
            CornerRadius = new CornerRadius(7), Background = new SolidColorBrush(Mt.TextC(0.06)), Opacity = 0,
            VerticalAlignment = VerticalAlignment.Top, Height = pitch - 1, Margin = new Thickness(-8, 0, -8, 0),
        };
        g.Children.Add(band);

        // Name line: 18 DIP frame, 1.5 low, 1 under the row's top.
        double nameTop = 1 + 1.5;
        double baseline = nameTop + Mt.BaselineIn(18, (13, Face.Text), (13, Face.Mono));
        var glyph = Icons.Glyph(s.Glyph, 14, 14 * s.GlyphRatio, Mt.Strong);
        g.Children.Add(Placed(glyph, 0, baseline - 4.5 - 7));
        g.Children.Add(Placed(Mt.AtBaseline(Mt.T(s.Name, 13, Face.Text, Wt.Medium, Mt.Text), baseline), 20, 0));

        var chips = new List<Meter>();
        if (s.Inner?.Remaining != null) chips.Add(s.Inner);
        if (s.Outer?.Remaining != null) chips.Add(s.Outer);
        double right = Gauges.OverviewWidth;
        for (int i = chips.Count - 1; i >= 0; i--)
        {
            var chip = MeterChip(chips[i].Label, chips[i].Remaining!.Value, Tone.Neutral);
            g.Children.Add(Placed(Mt.AtBaseline(chip, baseline, HorizontalAlignment.Right), right - 50, 0, 50));
            right -= 58;
        }

        g.Children.Add(Placed(Gauges.OverviewBar(Gauge.Of(s.Outer), Gauge.Of(s.Inner)), 0, 1 + 18 - 1));

        var comment = CommentText(s.Comment, Gauges.Toned(s.CommentTone, Mt.Secondary));
        double commentBaseline = 1 + 17 + Mt.BaselineIn(14, (11, Face.Text), (11, Face.Mono));
        var commentBox = Mt.AtBaseline(comment, commentBaseline, HorizontalAlignment.Right);
        commentBox.HorizontalAlignment = HorizontalAlignment.Stretch;
        g.Children.Add(commentBox);

        string name = $"{s.Name}，" + string.Join("，", chips.Select(c => $"{c.Label} 剩余 {c.Remaining}%")) + $"，{s.Comment}";
        var button = Bare(g, "row-" + s.Id, name, () => { _selected = s.Id; Render(); }, on => band.Opacity = on ? 1 : 0);
        button.HorizontalAlignment = HorizontalAlignment.Stretch;
        return button;
    }

    /// <summary>「5h 12%」: label mono 11, number mono 13 medium, the % dimmer unless toned.</summary>
    private static TextBlock MeterChip(string label, double value, Tone tone) => Mt.Block(
        Mt.R(label, 11, Face.Mono, Wt.Regular, Mt.Secondary),
        Mt.Gap(4),
        Mt.R(Percent(value), 13, Face.Mono, Wt.Medium, Gauges.Toned(tone, Mt.Text), -0.26),
        Mt.R("%", 13, Face.Mono, Wt.Regular, tone == Tone.Neutral ? Mt.Tertiary : Gauges.Toned(tone, Mt.Text)));

    private static string Percent(double v) => v > 0 && v < 1 ? "<1" : Math.Round(v).ToString("0");

    /// <summary>「超额 17%」: the number in the number face, 3 apart; other comments as they are.</summary>
    private static TextBlock CommentText(string text, Brush brush)
    {
        var parts = text.Split(' ', 2);
        var tb = parts.Length == 2 && parts[1].Any(char.IsDigit)
            ? Mt.Block(Mt.R(parts[0], 11), Mt.Gap(3), Mt.R(parts[1], 11, Face.Mono))
            : Mt.Block(Mt.R(text, 11));
        tb.Foreground = brush;
        return tb;
    }

    /// <summary>ModernQuotaDetail: the double ring on the left, header and allowance rows on the right.</summary>
    private UIElement QuotaDetail(Subscription s)
    {
        var g = new Grid { MinHeight = Mt.CardBodyHeight };

        // Ring centre: 44 light number + 22 light %, then 「7d 剩余」.
        var c = s.Center;
        var center = new Grid { Width = Mt.CardBodyHeight, Height = Mt.CardBodyHeight };
        if (c.Remaining is double rem)
        {
            double top = (Mt.CardBodyHeight - 60) / 2 + 1;
            var number = Mt.Block(
                Mt.R(Percent(rem), 44, Face.Mono, Wt.Light, Gauges.Toned(c.Tone, Mt.Text), -2.64),
                Mt.Gap(2),
                Mt.R("%", 22, Face.Mono, Wt.Light, Mt.Secondary));
            center.Children.Add(Placed(Mt.AtBaseline(number, top + Mt.BaselineIn(44, (44, Face.Mono)), HorizontalAlignment.Center), 0, 0, Mt.CardBodyHeight));
            var caption = Mt.Block(Mt.R(c.Label, 11, Face.Mono), Mt.Gap(4), Mt.R("剩余", 11, Face.Text, Wt.Regular, null, 1.1));
            caption.Foreground = Mt.Secondary;
            center.Children.Add(Placed(Mt.AtBaseline(caption, top + 44 + 2 + Mt.BaselineIn(14, (11, Face.Mono), (11, Face.Text)), HorizontalAlignment.Center), 0, 0, Mt.CardBodyHeight));
        }
        var ring = Gauges.QuotaRing(Gauge.Of(s.Outer), Gauge.Of(s.Inner), s.Outer == null && s.Inner == null, center);
        g.Children.Add(Placed(ring, -7, 0));

        // Header: mark, name, plan.
        double x0 = Mt.LeadingColumnWidth;
        double columnTop = Math.Max(0, (Mt.CardBodyHeight - (32 + 32 * s.Rows.Count)) / 2);
        double headerBaseline = columnTop + Mt.BaselineIn(22, (13, Face.Text), (11, Face.Text)) + 1.5;
        g.Children.Add(Placed(Icons.Glyph(s.Glyph, 15, 15 * s.GlyphRatio, Mt.Strong), x0, headerBaseline - 4.5 - 7.5));
        var title = Mt.Block(Mt.R(s.Name, 13, Face.Text, Wt.SemiBold, Mt.Text, -0.13));
        if (s.Plan != null)
        {
            title.Inlines.Add(Mt.Gap(7));
            title.Inlines.Add(Mt.R(s.Plan, 11, Face.Text, Wt.Regular, Mt.Secondary));
        }
        g.Children.Add(Placed(Mt.AtBaseline(title, headerBaseline), x0 + 22, 0));

        // Allowance rows: 32 DIP each under a rule, from 32 down. The column
        // (header and rows) is centred in the 148 body when shorter
        // (`.frame(minHeight: 148, alignment: .leading)`).
        double labelWidth = Math.Max(28, Math.Ceiling(s.Rows.Max(m => m.Label.Sum(ch => ch < 128 ? 6.6 : 11.0)) + 7));
        double y = 32 + columnTop;
        foreach (var m in s.Rows)
        {
            g.Children.Add(Placed(AllowanceRow(m, labelWidth), x0, y, Mt.RightColumnWidth, 32));
            y += 32;
        }
        return g;
    }

    private UIElement AllowanceRow(Meter m, double labelWidth)
    {
        var g = new Grid { Height = 32, Background = Mt.Transparent };
        var band = new Border
        {
            CornerRadius = new CornerRadius(6), Background = new SolidColorBrush(Mt.TextC(0.05)), Opacity = 0,
            Margin = new Thickness(-8, 1, -8, 0),
        };
        g.Children.Add(band);
        g.Children.Add(new Rectangle { Height = 1, Fill = Mt.Rule, VerticalAlignment = VerticalAlignment.Top });
        double baseline = 1 + Mt.BaselineIn(31, (11, Face.Mono), (13, Face.Mono), (11, Face.Text));
        g.Children.Add(Placed(Mt.AtBaseline(Mt.T(m.Label, 11, Face.Mono, Wt.Regular, Mt.Secondary), baseline), 0, 0));
        if (m.Remaining is double rem)
        {
            var value = Mt.Block(Mt.R(Percent(rem), 13, Face.Mono, Wt.Medium, Mt.Text, -0.26), Mt.R("%", 13, Face.Mono, Wt.Regular, Mt.Tertiary));
            g.Children.Add(Placed(Mt.AtBaseline(value, baseline), labelWidth, 0));
            if (m.Prediction != null)
                g.Children.Add(Placed(Mt.AtBaseline(Mt.T(m.Prediction, 11, Face.Text, Wt.Regular, Gauges.Toned(m.Tone, Mt.Secondary)), baseline), labelWidth + 52, 0));
            if (m.Reset != null)
            {
                var reset = Mt.Mixed(m.Reset, 11, Mt.Secondary);
                var resetBox = Mt.AtBaseline(reset, baseline, HorizontalAlignment.Right);
                resetBox.HorizontalAlignment = HorizontalAlignment.Stretch;
                reset.Margin = new Thickness(0, -80, 0, 0);
                g.Children.Add(resetBox);
                var icon = Icons.Fluent(Icons.Clockwise, 10, Mt.Secondary);
                icon.HorizontalAlignment = HorizontalAlignment.Right;
                icon.VerticalAlignment = VerticalAlignment.Top;
                icon.Width = 12;
                icon.Height = 12;
                reset.Measure(new Windows.Foundation.Size(double.PositiveInfinity, double.PositiveInfinity));
                icon.Margin = new Thickness(0, baseline - 10, reset.DesiredSize.Width + 4 - 1, 0);
                g.Children.Add(icon);
            }
        }
        g.PointerEntered += (_, _) => band.Opacity = 1;
        g.PointerExited += (_, _) => band.Opacity = 0;
        if (m.Remaining is double r)
            ToolTipService.SetToolTip(g, $"{(m.Label == "5h" ? "5 小时" : m.Label == "7d" ? "7 天" : m.Label == "mo" ? "本月" : m.Label)}窗口\n已使用 {100 - Math.Round(r)}%");
        return g;
    }

    // -- 近期消耗 ----------------------------------------------------------------------------------

    private UIElement UsageCard()
    {
        var p = FakeData.Periods[_period];
        double share = Share;

        // Title row: quick periods, a rule, the filter icon (its padding hangs past the text edge).
        var controls = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 2, Margin = new Thickness(0, 0, -7, 0) };
        for (int i = 0; i < FakeData.Periods.Count; i++)
        {
            int index = i;
            var period = FakeData.Periods[i];
            bool on = i == _period;
            var text = Mt.T(period.Chip, 11, period.ChipMono ? Face.Mono : Face.Text, on ? Wt.Medium : Wt.Regular, on ? Mt.Text : Mt.Secondary);
            var chip = new Border
            {
                Height = 20, Padding = new Thickness(8, 0, 8, 0), CornerRadius = new CornerRadius(10),
                Background = on ? Mt.Chip : Mt.Transparent,
                Child = Mt.AtBaseline(text, Mt.BaselineIn(20, (11, period.ChipMono ? Face.Mono : Face.Text))),
            };
            controls.Children.Add(Bare(chip, "period-" + i, period.Chip, () => { _period = index; Render(); },
                on ? null : Lights(Mt.Secondary, b => text.Foreground = b)));
        }
        var rule = VRule(12, 0.16);
        rule.Margin = new Thickness(4, 0, 4, 0);
        controls.Children.Add(rule);
        var filterIcon = Icons.Filter(Mt.Secondary);
        var filterBox = new Border { Padding = new Thickness(6, 0, 6, 0), CornerRadius = new CornerRadius(10), Background = Mt.Transparent, Child = filterIcon };
        var filter = Bare(filterBox, "filter", "筛选", () => { }, Lights(Mt.Secondary, b =>
        {
            foreach (var path in ((Canvas)filterIcon).Children.OfType<Path>()) path.Stroke = b;
        }), "筛选");
        filter.Flyout = FilterMenu();
        controls.Children.Add(filter);

        // Body: big number and three rows on the left, the curve on the right.
        var g = new Grid { Height = Mt.CardBodyHeight };
        g.Children.Add(Placed(Mt.AtBaseline(BigCost(p.Cost * share), Mt.BaselineIn(44, (44, Face.Mono))), 0, 0));
        g.Children.Add(Placed(Mt.AtBaseline(Mt.T("预估费用", 11, Face.Text, Wt.Regular, Mt.Secondary), 44 + Mt.BaselineIn(18, (11, Face.Text))), 0, 0));
        AddUsageRow(g, "Token", FakeData.PanelTokens(p.TokensM * share), 0);
        AddUsageRow(g, "活跃", p.Active, 1);
        AddUsageRow(g, "AI 工时", p.Ai, 2);
        g.Children.Add(Placed(UsagePlot.Build(p, share, Mt.RightColumnWidth), Mt.LeadingColumnWidth, 0));
        return CardFrame("近期消耗", controls, g);
    }

    private static MenuFlyout FilterMenu()
    {
        var menu = new MenuFlyout { Placement = Microsoft.UI.Xaml.Controls.Primitives.FlyoutPlacementMode.BottomEdgeAlignedRight };
        foreach (var (title, value) in new[] { ("时间", "今天"), ("终端", "全部"), ("工具", "全部"), ("模型", "全部") })
            menu.Items.Add(new MenuFlyoutItem { Text = $"{title}　{value}" });
        return menu;
    }

    /// <summary>「$41.20」: the sign 22 light at 50 %, the integer 44 light, the cents 22 light at 62 %.</summary>
    private static TextBlock BigCost(double dollars)
    {
        string text = FakeData.CostCenter(dollars).TrimStart('$');
        string integer = text, fraction = "";
        int dot = text.IndexOf('.');
        if (dot >= 0)
        {
            integer = text[..dot];
            fraction = text[(dot + 1)..];
        }
        var tb = Mt.Block(Mt.R("$", 22, Face.Mono, Wt.Light, Mt.Tertiary), Mt.Gap(1));
        if (fraction.Length > 0 && integer.Length > 0)
        {
            // The integer's last digit carries the -3 before the point.
            if (integer.Length > 1) tb.Inlines.Add(Mt.R(integer[..^1], 44, Face.Mono, Wt.Light, Mt.Text, -2.64));
            tb.Inlines.Add(Mt.R(integer[^1..], 44, Face.Mono, Wt.Light, Mt.Text, -2.64 - 3));
            tb.Inlines.Add(Mt.R(".", 22, Face.Mono, Wt.Light, Mt.Secondary, -4));
            tb.Inlines.Add(Mt.R(fraction, 22, Face.Mono, Wt.Light, Mt.Secondary, -1.1));
        }
        else
        {
            tb.Inlines.Add(Mt.R(integer, 44, Face.Mono, Wt.Light, Mt.Text, -2.64));
        }
        return tb;
    }

    /// <summary>RecentUsageRowLabel: 11 label in a 36 column, 10, the value mono 13 medium; centred, not baseline-aligned.</summary>
    private static void AddUsageRow(Grid g, string label, string value, int index)
    {
        double top = Mt.CardBodyHeight - 54 + index * 18;
        g.Children.Add(Placed(Mt.AtBaseline(Mt.T(label, 11, Face.Text, Wt.Regular, Mt.Secondary), top + Mt.BaselineIn(18, (11, Face.Text))), 0, 0));
        g.Children.Add(Placed(Mt.AtBaseline(Mt.Words(value, 13, 4.5, Face.Mono, Wt.Medium, Mt.Text), top + Mt.BaselineIn(18, (13, Face.Mono))), 46, 0));
    }

    // -- 展开详情 ----------------------------------------------------------------------------------

    private UIElement DetailsGrid()
    {
        var g = new Grid { ColumnSpacing = Mt.CardSpacing, RowSpacing = Mt.CardSpacing };
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(244) });
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(244) });
        g.RowDefinitions.Add(new RowDefinition { Height = new GridLength(252) });
        g.RowDefinitions.Add(new RowDefinition { Height = new GridLength(252) });
        for (int i = 0; i < FakeData.DetailCards.Count; i++)
        {
            var card = DetailCard(i);
            Grid.SetColumn(card, i % 2);
            Grid.SetRow(card, i / 2);
            g.Children.Add(card);
        }
        return g;
    }

    private FrameworkElement DetailCard(int index)
    {
        var data = FakeData.DetailCards[index];
        bool byCost = data.HasSwitch && _byCost[index];
        var p = FakeData.Periods[_period];
        var today = FakeData.Periods[0];
        double tokenScale = p.TokensM / today.TokensM * Share;
        double costScale = p.Cost / today.Cost * Share;
        double Val(RankRow r) => byCost ? r.Cost * costScale : r.TokensM * tokenScale;

        var ordered = data.FixedOrder ? data.Rows.ToList() : data.Rows.OrderByDescending(Val).ToList();
        double total = ordered.Sum(Val);
        // Top three, then 「其余 N 个」 for two or more left (a lone fourth is listed).
        int listedCount = ordered.Count - 3 >= 2 ? 3 : ordered.Count;
        var listed = ordered.Take(listedCount).ToList();
        var rest = ordered.Skip(listedCount).ToList();
        Color[] rank = { Mt.RankBlue, Mt.RankTeal, Mt.RankMagenta };

        string Fmt(double v) => byCost ? FakeData.Cost(v) : FakeData.Tokens(v, total);

        // Metric switch: Token / 费用, the period chips' look; its text edge on the 28 line.
        UIElement? accessory = null;
        if (data.HasSwitch)
        {
            var sw = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 2, Margin = new Thickness(0, 0, -8, 0) };
            foreach (var (label, cost) in new[] { ("Token", false), ("费用", true) })
            {
                bool on = cost == byCost;
                var face = cost ? Face.Text : Face.Mono;
                var text = Mt.T(label, 11, face, on ? Wt.Medium : Wt.Regular, on ? Mt.Text : Mt.Secondary);
                var chip = new Border
                {
                    Height = 20, Padding = new Thickness(8, 0, 8, 0), CornerRadius = new CornerRadius(10),
                    Background = on ? Mt.Chip : Mt.Transparent, Child = Mt.AtBaseline(text, Mt.BaselineIn(20, (11, face))),
                };
                sw.Children.Add(Bare(chip, $"metric-{index}-{label}", $"{data.Title} {label}", () => { _byCost[index] = cost; Render(); },
                    on ? null : Lights(Mt.Secondary, b => text.Foreground = b)));
            }
            accessory = sw;
        }

        var body = new Grid { Height = 252 - 2 * Mt.CardPadding - 20 - 8 };
        var segments = listed.Select((r, i) => (Val(r), rank[Math.Min(i, 2)])).ToList();
        if (rest.Count > 0) segments.Add((rest.Sum(Val), Mt.RankGray));
        var ring = new Grid { Width = Gauges.DistributionSize, Height = Gauges.DistributionSize, HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Top, Margin = new Thickness(0, 4, 0, 0) };
        ring.Children.Add(Gauges.DistributionRing(segments));
        string centerText = byCost ? FakeData.CostCenter(total) : FakeData.Tokens(total, total);
        ring.Children.Add(Mt.AtBaseline(Mt.T(centerText, 13, Face.Mono, Wt.Medium, Mt.Text), 29 + Mt.BaselineIn(16, (13, Face.Mono)), HorizontalAlignment.Center));
        var unit = Mt.T(byCost ? "费用" : "Token", 11, byCost ? Face.Text : Face.Mono, Wt.Regular, Mt.Secondary);
        ring.Children.Add(Mt.AtBaseline(unit, 45 + Mt.BaselineIn(14, (11, byCost ? Face.Text : Face.Mono)), HorizontalAlignment.Center));
        body.Children.Add(ring);

        double rowTop = 4 + Gauges.DistributionSize + 12;
        double contentWidth = 244 - 2 * Mt.CardPadding;
        for (int i = 0; i < listed.Count; i++)
        {
            var r = listed[i];
            body.Children.Add(Placed(DetailRow(r.Name, null, Fmt(Val(r)), total > 0 ? Val(r) / total : 0, rank[Math.Min(i, 2)], data.NamesAreDynamic, contentWidth), 0, rowTop + i * 22, contentWidth, 22));
        }
        if (rest.Count > 0)
        {
            double restValue = rest.Sum(Val);
            var row = DetailRow("其余 ", rest.Count.ToString(), Fmt(restValue), total > 0 ? restValue / total : 0, Mt.RankGray, false, contentWidth);
            var tip = string.Join("\n", rest.Select(r => $"{r.Name}  {Fmt(Val(r))}"));
            ToolTipService.SetToolTip(row, tip);
            body.Children.Add(Placed(row, 0, rowTop + listed.Count * 22, contentWidth, 22));
        }
        return CardFrame(data.Title, accessory, body);
    }

    /// <summary>A ranked row: 7 DIP swatch, 7, the name, the value, the share right-aligned in 32.</summary>
    private static Grid DetailRow(string name, string? count, string value, double share, Color swatch, bool truncate, double width)
    {
        var g = new Grid { Height = 22, Width = width, Background = Mt.Transparent };
        g.Children.Add(new Rectangle
        {
            Width = 7, Height = 7, RadiusX = 2, RadiusY = 2, Fill = new SolidColorBrush(swatch),
            HorizontalAlignment = HorizontalAlignment.Left, VerticalAlignment = VerticalAlignment.Top, Margin = new Thickness(0, 7.5, 0, 0),
        });
        double baseline = Mt.BaselineIn(22, (13, Face.Text), (13, Face.Mono), (11, Face.Mono));
        var shareText = Mt.T(FakeData.Share(share), 11, Face.Mono, Wt.Regular, Mt.Secondary);
        g.Children.Add(Placed(Mt.AtBaseline(shareText, baseline, HorizontalAlignment.Right), width - 32, 0, 32));
        var valueText = Mt.T(value, 13, Face.Mono, Wt.Medium, Mt.Text, -0.26);
        var valueBox = Mt.AtBaseline(valueText, baseline, HorizontalAlignment.Right);
        valueBox.HorizontalAlignment = HorizontalAlignment.Stretch;
        valueBox.Margin = new Thickness(0, 0, 32, 0);
        g.Children.Add(valueBox);
        valueText.Measure(new Windows.Foundation.Size(double.PositiveInfinity, double.PositiveInfinity));
        double nameMax = Math.Max(20, width - 14 - 32 - valueText.DesiredSize.Width - 8);
        var nameText = count == null
            ? Mt.T(truncate ? MiddleTruncate(name, nameMax) : name, 13, Face.Text, Wt.Medium, Mt.Text)
            : Mt.Block(Mt.R(name, 13, Face.Text, Wt.Medium), Mt.R(count, 13, Face.Mono, Wt.Medium), Mt.R(" 个", 13, Face.Text, Wt.Medium));
        if (count != null) nameText.Foreground = Mt.Secondary;
        nameText.TextTrimming = TextTrimming.CharacterEllipsis;
        nameText.MaxWidth = nameMax;
        g.Children.Add(Placed(Mt.AtBaseline(nameText, baseline), 14, 0));
        if (truncate) ToolTipService.SetToolTip(g, name);
        return g;
    }

    /// <summary>Middle truncation for long names (「Studio-…Book-Pro-14」), by an average glyph width.</summary>
    private static string MiddleTruncate(string name, double maxWidth)
    {
        double avg = 7.4;
        int fit = (int)(maxWidth / avg);
        if (name.Length <= fit || fit < 5) return name;
        int head = (fit - 1) / 2, tail = fit - 1 - head;
        return name[..head] + "…" + name[^tail..];
    }

    // -- footer (ModernFooterBar) -------------------------------------------------------------------

    private UIElement Footer()
    {
        var g = new Grid { Height = 38, Padding = new Thickness(Mt.TextInset, 10, Mt.TextInset, 10) };

        var label = Mt.T(_expanded ? "收起详情" : "展开详情", 13, Face.Text, Wt.Regular, Mt.Text);
        var chevron = (Canvas)Icons.Chevron(_expanded, new SolidColorBrush(Mt.TextC(0.7)));
        chevron.VerticalAlignment = VerticalAlignment.Top;
        chevron.Margin = new Thickness(8, 9 - chevron.Height / 2, 0, 0);
        var left = new StackPanel { Orientation = Orientation.Horizontal };
        left.Children.Add(Mt.AtBaseline(label, Mt.BaselineIn(18, (13, Face.Text))));
        left.Children.Add(chevron);
        g.Children.Add(Bare(Sized(left, 18), "details", _expanded ? "收起详情" : "展开详情", () => { _expanded = !_expanded; Render(); }));

        var right = new StackPanel { Orientation = Orientation.Horizontal, HorizontalAlignment = HorizontalAlignment.Right };
        var sync = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6 };
        double syncBaseline = Mt.BaselineIn(18, (11, Face.Mono), (11, Face.Text));
        TextBlock syncText;
        if (_syncing)
        {
            sync.Children.Add(Icons.Spinner(12));
            syncText = Mt.T("同步中", 11, Face.Text, Wt.Regular, Mt.Secondary);
        }
        else
        {
            var icon = Icons.Sort(Mt.Secondary);
            sync.Children.Add(icon);
            syncText = Mt.Block(Mt.R(FakeData.LastSyncNumber, 11, Face.Mono), Mt.R(FakeData.LastSyncText, 11));
            syncText.Foreground = Mt.Secondary;
        }
        sync.Children.Add(Mt.AtBaseline(syncText, syncBaseline));
        var syncButton = Bare(Sized(sync, 18), "sync", "同步用量", StartFakeSync, _syncing ? null : Lights(Mt.Secondary,
            b => syncText.Foreground = b,
            b => { foreach (var path in ((Canvas)sync.Children[0]).Children.OfType<Path>()) path.Stroke = b; }), "同步用量");
        right.Children.Add(syncButton);
        var sep = VRule(12);
        sep.Margin = new Thickness(10, 0, 10, 0);
        right.Children.Add(sep);
        var power = Icons.Fluent(Icons.Power, 10, Mt.Secondary);
        var powerBox = new Grid { Width = 12, Height = 18, Background = Mt.Transparent };
        powerBox.Children.Add(power);
        right.Children.Add(Bare(powerBox, "quit", "退出 Vibe Usage", () => QuitRequested?.Invoke(), Lights(Mt.Secondary, b => power.Foreground = b), "退出 Vibe Usage"));
        g.Children.Add(right);
        return g;
    }
}
