using System;
using System.Linq;
using System.Numerics;
using System.Threading.Tasks;
using Microsoft.UI.Composition;
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
using Windows.System;
using WinRT.Interop;

namespace VibeUsage.PanelPoc;

/// <summary>
/// The tray panel. Window chrome: no title bar, a thin system border (which
/// gives the Windows 11 rounded corners and shadow), always on top, never in
/// the taskbar or Alt+Tab, Desktop Acrylic behind. Show/hide and focus rules
/// live in <see cref="FlyoutController"/>; this class only draws.
/// </summary>
public sealed partial class PanelWindow : Window
{
    public const double PanelWidthDip = 380;

    private readonly FlyoutBackdrop? _backdrop;
    private string _tab = "overview";
    private int _rangeIndex;
    private int _dimensionIndex;
    private bool _byCost;
    private bool _syncing;
    private bool _refreshing;

    public IntPtr Hwnd { get; }

    /// <summary>Set before quitting so Alt+F4 handling doesn't veto the real close.</summary>
    public bool AllowClose { get; set; }

    public event Action<string>? CloseRequested;
    public event Action? SettingsRequested;
    public event Action? ContentHeightChanged;

    internal PanelWindow(PocOptions options)
    {
        InitializeComponent();
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

        if (FlyoutBackdrop.IsSupported)
        {
            _backdrop = new FlyoutBackdrop();
            SystemBackdrop = _backdrop;
        }
        else
        {
            RootGrid.Background = (Brush)R("SolidBackgroundFillColorBaseBrush");
        }

        RootGrid.ActualThemeChanged += (_, _) => ApplyTheme();
        ContentStack.SizeChanged += (_, _) => ContentHeightChanged?.Invoke();

        TabBar.Items.Add(new SelectorBarItem { Text = "概览", Tag = "overview" });
        foreach (var s in FakeData.Subscriptions) TabBar.Items.Add(new SelectorBarItem { Text = s.Name, Tag = s.Id });
        var startTab = TabBar.Items.FirstOrDefault(i => (string)i.Tag == options.Tab) ?? TabBar.Items[0];
        _tab = (string)startTab.Tag;
        TabBar.SelectedItem = startTab;

        RangeBox.ItemsSource = FakeData.Ranges.Select(r => r.Label).ToList();
        RangeBox.SelectedIndex = 0;

        foreach (var d in FakeData.Dimensions) DimensionBar.Items.Add(new SelectorBarItem { Text = d.Label });
        DimensionBar.SelectedItem = DimensionBar.Items[0];
        MetricBox.ItemsSource = new[] { "Token", "费用" };
        MetricBox.SelectedIndex = 0;

        DetailsExpander.IsExpanded = options.Expand;
        SyncText.Text = FakeData.LastSync;

        ApplyTheme();
        RenderAll();
    }

    // -- window behaviour ---------------------------------------------------------------

    /// <summary>Natural height of the whole panel at the fixed width, in DIPs.</summary>
    public double MeasureDesiredHeightDip()
    {
        RootGrid.Measure(new Windows.Foundation.Size(PanelWidthDip, double.PositiveInfinity));
        return Math.Ceiling(RootGrid.DesiredSize.Height);
    }

    public void PrepareForOpen()
    {
        Scroller.ChangeView(null, 0, null, true);
    }

    /// <summary>
    /// Windows 11 flyouts slide in from the taskbar and fade up (Fluent
    /// "decelerate" curve). The window itself appears through DWM's own show
    /// transition; the content slides on top of that.
    /// </summary>
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
            var linear = compositor.CreateLinearEasingFunction();

            var slide = compositor.CreateVector3KeyFrameAnimation();
            slide.InsertKeyFrame(0f, from);
            slide.InsertKeyFrame(1f, Vector3.Zero, decelerate);
            slide.Duration = TimeSpan.FromMilliseconds(300);

            var fade = compositor.CreateScalarKeyFrameAnimation();
            fade.InsertKeyFrame(0f, 0f);
            fade.InsertKeyFrame(1f, 1f, linear);
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
        if (keyboard && TabBar.SelectedItem is SelectorBarItem item)
            item.Focus(FocusState.Keyboard);
        else
            FocusSink.Focus(FocusState.Programmatic);
    }

    private void ApplyTheme()
    {
        var theme = RootGrid.ActualTheme;
        Win32.SetDwmInt(Hwnd, Win32.DWMWA_USE_IMMERSIVE_DARK_MODE, theme == ElementTheme.Dark ? 1 : 0);
        _backdrop?.SetTheme(theme);
        PocLog.Write($"theme: {theme}");
    }

    private void OnRootKeyDown(object sender, KeyRoutedEventArgs e)
    {
        if (e.Key == VirtualKey.Escape)
        {
            e.Handled = true;
            CloseRequested?.Invoke("esc");
        }
    }

    // -- interactions ---------------------------------------------------------------------

    private void OnTabChanged(SelectorBar sender, SelectorBarSelectionChangedEventArgs args)
    {
        if (sender.SelectedItem?.Tag is string tab && tab != _tab)
        {
            _tab = tab;
            RenderQuota();
            RenderUsage();
        }
    }

    private void OnRangeChanged(object sender, SelectionChangedEventArgs e)
    {
        if (RangeBox.SelectedIndex < 0) return;
        _rangeIndex = RangeBox.SelectedIndex;
        RenderUsage();
    }

    private void OnDimensionChanged(SelectorBar sender, SelectorBarSelectionChangedEventArgs args)
    {
        int i = sender.SelectedItem is null ? 0 : sender.Items.IndexOf(sender.SelectedItem);
        if (i < 0 || i == _dimensionIndex) return;
        _dimensionIndex = i;
        RenderDimension();
    }

    private void OnMetricChanged(object sender, SelectionChangedEventArgs e)
    {
        _byCost = MetricBox.SelectedIndex == 1;
        RenderDimension();
    }

    private void OnSyncClick(object sender, RoutedEventArgs e) => StartFakeSync();

    private void OnSettingsClick(object sender, RoutedEventArgs e) => SettingsRequested?.Invoke();

    private void OnWebClick(object sender, RoutedEventArgs e)
    {
        // The sample never goes online; the real app would open the browser here.
        PocLog.Write("web/leaderboard clicked (sample: no browser)");
    }

    private async void OnRefreshQuotasClick(object sender, RoutedEventArgs e)
    {
        if (_refreshing) return;
        _refreshing = true;
        RefreshGlyph.Visibility = Visibility.Collapsed;
        RefreshRing.Visibility = Visibility.Visible;
        RefreshRing.IsActive = true;
        await Task.Delay(1200);
        RefreshRing.IsActive = false;
        RefreshRing.Visibility = Visibility.Collapsed;
        RefreshGlyph.Visibility = Visibility.Visible;
        _refreshing = false;
    }

    /// <summary>Pretend to sync (tray menu 「立即同步」 and the footer button).</summary>
    public async void StartFakeSync()
    {
        if (_syncing) return;
        _syncing = true;
        SyncGlyph.Visibility = Visibility.Collapsed;
        SyncRing.Visibility = Visibility.Visible;
        SyncRing.IsActive = true;
        SyncText.Text = "正在同步…";
        await Task.Delay(1500);
        SyncRing.IsActive = false;
        SyncRing.Visibility = Visibility.Collapsed;
        SyncGlyph.Visibility = Visibility.Visible;
        SyncText.Text = "刚刚同步";
        _syncing = false;
    }

    // -- rendering ------------------------------------------------------------------------

    private static readonly System.Collections.Generic.Dictionary<string, object> ResourceCache = new();

    /// <summary>
    /// An app resource by key. The projected dictionary lookup can miss keys
    /// that XAML itself resolves (seen in CI: TryGetValue said "missing" for
    /// keys that {StaticResource} found), so fall back to letting XAML resolve it.
    /// </summary>
    internal static object R(string key)
    {
        if (ResourceCache.TryGetValue(key, out var cached)) return cached;
        object? value = null;
        try
        {
            value = Application.Current.Resources[key];
        }
        catch (Exception)
        {
        }
        if (value is null)
        {
            var probe = (ContentControl)XamlReader.Load(
                "<ContentControl xmlns=\"http://schemas.microsoft.com/winfx/2006/xaml/presentation\" Tag=\"{StaticResource " + key + "}\" />");
            value = probe.Tag;
        }
        ResourceCache[key] = value;
        return value;
    }

    private static Style S(string key) => (Style)R(key);

    private static TextBlock Text(string text, string style, TextAlignment align = TextAlignment.Left) => new()
    {
        Text = text,
        Style = S(style),
        TextAlignment = align,
        TextTrimming = TextTrimming.CharacterEllipsis,
        TextWrapping = TextWrapping.NoWrap,
        VerticalAlignment = VerticalAlignment.Center,
    };

    private static Rectangle Divider(Thickness margin) => new() { Style = S("PanelDividerStyle"), Margin = margin };

    private static FrameworkElement Glyph(string data, double size)
    {
        var path = (Path)XamlReader.Load(
            "<Path xmlns=\"http://schemas.microsoft.com/winfx/2006/xaml/presentation\" Data=\"" + data + "\" />");
        path.Style = S("PanelGlyphPathStyle");
        var box = new Grid { Width = 24, Height = 24 };
        box.Children.Add(path);
        return new Viewbox { Width = size, Height = size, Child = box, VerticalAlignment = VerticalAlignment.Center };
    }

    private static string StatusStyle(Pace pace) => pace switch
    {
        Pace.Critical => "PanelCaptionCriticalStyle",
        Pace.Fast => "PanelCaptionCautionStyle",
        _ => "PanelCaptionSecondaryStyle",
    };

    private static string BarStyle(Pace pace) => pace switch
    {
        Pace.Critical => "PanelBarCriticalStyle",
        Pace.Fast => "PanelBarCautionStyle",
        _ => "PanelBarStyle",
    };

    private static string StatusText(QuotaWindow w) => w.Pace == Pace.Fast ? "偏快 · " + w.Prediction : w.Prediction;

    private static void Place(FrameworkElement e, int row = 0, int column = 0, int columnSpan = 1)
    {
        Grid.SetRow(e, row);
        Grid.SetColumn(e, column);
        Grid.SetColumnSpan(e, columnSpan);
    }

    private static Grid Columns(params GridLength[] widths)
    {
        var g = new Grid();
        foreach (var w in widths) g.ColumnDefinitions.Add(new ColumnDefinition { Width = w });
        return g;
    }

    private static readonly GridLength Auto = new(1, GridUnitType.Auto);
    private static readonly GridLength Star = new(1, GridUnitType.Star);
    private static GridLength Px(double v) => new(v);

    private void RenderAll()
    {
        RenderQuota();
        RenderUsage();
    }

    private Subscription? CurrentSubscription => FakeData.Subscriptions.FirstOrDefault(s => s.Id == _tab);

    private void RenderQuota()
    {
        QuotaHost.Children.Clear();
        var sub = CurrentSubscription;
        if (sub is null)
        {
            int critical = FakeData.Subscriptions.Count(s => s.Pace == Pace.Critical);
            int fast = FakeData.Subscriptions.Count(s => s.Pace == Pace.Fast);
            QuotaSummary.Text = critical + fast == 0
                ? "都在正常进度"
                : string.Join("，", new[] { critical > 0 ? $"{critical} 个快用完" : null, fast > 0 ? $"{fast} 个偏快" : null }.Where(x => x != null));
            for (int i = 0; i < FakeData.Subscriptions.Count; i++)
            {
                if (i > 0) QuotaHost.Children.Add(Divider(new Thickness(12, 1, 12, 1)));
                QuotaHost.Children.Add(OverviewRow(FakeData.Subscriptions[i]));
            }
        }
        else
        {
            QuotaSummary.Text = string.Empty;
            QuotaHost.Children.Add(DetailBlock(sub));
        }
    }

    /// <summary>
    /// One subscription in 概览, two lines: name and its first two windows'
    /// remaining share; then the bar of the window that matters and how it is
    /// going. Click (or Enter) opens the subscription's tab.
    /// </summary>
    private Button OverviewRow(Subscription s)
    {
        var b = s.Binding;
        var grid = new Grid { RowSpacing = 5 };
        grid.RowDefinitions.Add(new RowDefinition { Height = Auto });
        grid.RowDefinitions.Add(new RowDefinition { Height = Auto });

        var top = Columns(Auto, Star, Auto);
        var name = Text(s.Name, "PanelBodyStrongStyle");
        name.Margin = new Thickness(10, 0, 0, 0);
        var values = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 12 };
        foreach (var w in s.Windows.Take(2))
        {
            var pair = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 4 };
            pair.Children.Add(Text(w.Label, "PanelCaptionSecondaryStyle"));
            pair.Children.Add(Text($"{w.Remaining}%", "PanelBodyStrongStyle"));
            values.Children.Add(pair);
        }
        Place(name, column: 1);
        Place(values, column: 2);
        top.Children.Add(Glyph(s.Glyph, 16));
        top.Children.Add(name);
        top.Children.Add(values);
        grid.Children.Add(top);

        var bottom = Columns(Star, Auto);
        bottom.Margin = new Thickness(26, 0, 0, 0);
        bottom.ColumnSpacing = 12;
        var bar = new ProgressBar { Style = S(BarStyle(b.Pace)), Value = b.Remaining };
        AutomationProperties.SetName(bar, $"{b.Label}剩余 {b.Remaining}%");
        var status = Text(StatusText(b), StatusStyle(b.Pace));
        Place(status, column: 1);
        bottom.Children.Add(bar);
        bottom.Children.Add(status);
        Place(bottom, row: 1);
        grid.Children.Add(bottom);

        var button = new Button { Style = S("PanelRowButtonStyle"), Content = grid };
        AutomationProperties.SetName(button,
            $"{s.Name}，" + string.Join("，", s.Windows.Select(w => $"{w.Label}剩余 {w.Remaining}%")) + $"，{StatusText(b)}");
        ToolTipService.SetToolTip(button, $"{s.Name} {s.Plan} · {b.Label}{b.Reset}");
        button.Click += (_, _) =>
        {
            var item = TabBar.Items.FirstOrDefault(i => (string)i.Tag == s.Id);
            if (item != null) TabBar.SelectedItem = item;
        };
        return button;
    }

    /// <summary>One subscription's tab: a gauge for the window that matters, then every window.</summary>
    private FrameworkElement DetailBlock(Subscription s)
    {
        var b = s.Binding;
        var stack = new StackPanel { Padding = new Thickness(12, 10, 12, 8) };

        var header = Columns(Auto, Auto, Star);
        var name = Text(s.Name, "PanelBodyStrongStyle");
        name.Margin = new Thickness(10, 0, 0, 0);
        var plan = Text(s.Plan, "PanelCaptionTertiaryStyle");
        plan.Margin = new Thickness(6, 1, 0, 0);
        Place(name, column: 1);
        Place(plan, column: 2);
        header.Children.Add(Glyph(s.Glyph, 16));
        header.Children.Add(name);
        header.Children.Add(plan);
        stack.Children.Add(header);

        var summary = Columns(Auto, Star);
        summary.Margin = new Thickness(0, 14, 0, 14);
        var gauge = new Grid { Width = 76, Height = 76 };
        string ringStyle = b.Pace switch
        {
            Pace.Critical => "PanelRingCriticalStyle",
            Pace.Fast => "PanelRingCautionStyle",
            _ => "PanelRingStyle",
        };
        gauge.Children.Add(new ProgressRing { Style = S(ringStyle), Value = b.Remaining, Width = 76, Height = 76 });
        var pct = Text($"{b.Remaining}%", "PanelBodyStrongStyle", TextAlignment.Center);
        pct.FontSize = 20;
        pct.HorizontalAlignment = HorizontalAlignment.Center;
        gauge.Children.Add(pct);
        var lines = new StackPanel { Margin = new Thickness(16, 0, 0, 0), Spacing = 2, VerticalAlignment = VerticalAlignment.Center };
        lines.Children.Add(Text($"{b.Label}额度剩余 {b.Remaining}%", "PanelCaptionSecondaryStyle"));
        lines.Children.Add(Text(StatusText(b), b.Pace switch
        {
            Pace.Critical => "PanelBodyStrongCriticalStyle",
            Pace.Fast => "PanelBodyStrongCautionStyle",
            _ => "PanelBodyStrongStyle",
        }));
        lines.Children.Add(Text(b.Reset, "PanelCaptionTertiaryStyle"));
        Place(lines, column: 1);
        summary.Children.Add(gauge);
        summary.Children.Add(lines);
        stack.Children.Add(summary);

        stack.Children.Add(Divider(new Thickness(-12, 0, -12, 6)));

        foreach (var w in s.Windows)
        {
            var row = new Grid { RowSpacing = 2, Margin = new Thickness(0, 6, 0, 6) };
            row.RowDefinitions.Add(new RowDefinition { Height = Auto });
            row.RowDefinitions.Add(new RowDefinition { Height = Auto });
            var top = Columns(Px(52), Star, Px(44));
            var bar = new ProgressBar { Style = S(BarStyle(w.Pace)), Value = w.Remaining };
            AutomationProperties.SetName(bar, $"{w.Label}剩余 {w.Remaining}%");
            var value = Text($"{w.Remaining}%", "PanelBodyStyle", TextAlignment.Right);
            Place(bar, column: 1);
            Place(value, column: 2);
            top.Children.Add(Text(w.Label, "PanelCaptionSecondaryStyle"));
            top.Children.Add(bar);
            top.Children.Add(value);
            var bottom = Columns(Star, Auto);
            bottom.Margin = new Thickness(52, 0, 0, 0);
            var reset = Text(w.Reset, "PanelCaptionTertiaryStyle");
            Place(reset, column: 1);
            bottom.Children.Add(Text(StatusText(w), StatusStyle(w.Pace)));
            bottom.Children.Add(reset);
            Place(bottom, row: 1);
            row.Children.Add(top);
            row.Children.Add(bottom);
            stack.Children.Add(row);
        }
        return stack;
    }

    private void RenderUsage()
    {
        var r = FakeData.Ranges[_rangeIndex];
        double share = CurrentSubscription?.UsageShare ?? 1.0;
        CostText.Text = FakeData.Money(r.Cost * share);
        TokensText.Text = FakeData.Tokens(r.TokensM * share);
        ActiveText.Text = r.Active;
        AiText.Text = r.AiHours;
        RenderChart(r, share);
        RenderDimension();
    }

    /// <summary>
    /// Accent-coloured bars with a faint peak line, the way Windows Settings
    /// draws battery and data usage. Hover a bar for its value.
    /// </summary>
    private void RenderChart(UsageRange r, double share)
    {
        ChartArea.Children.Clear();
        ChartArea.RowDefinitions.Clear();
        ChartArea.RowDefinitions.Add(new RowDefinition { Height = Auto });
        ChartArea.RowDefinitions.Add(new RowDefinition { Height = Star });

        int n = r.Bars.Length;
        double max = r.Bars.Max() * share;

        var peak = Columns(Star, Auto);
        var peakLine = Divider(new Thickness(0, 0, 8, 0));
        peakLine.Opacity = 0.7;
        peakLine.VerticalAlignment = VerticalAlignment.Bottom;
        var peakLabel = Text(FakeData.Money(max), "PanelCaptionTertiaryStyle");
        Place(peakLabel, column: 1);
        peak.Children.Add(peakLine);
        peak.Children.Add(peakLabel);
        ChartArea.Children.Add(peak);

        const double plotHeight = 62;
        var plot = new Grid { Height = plotHeight, VerticalAlignment = VerticalAlignment.Bottom };
        Place(plot, row: 1);
        for (int i = 0; i < n; i++) plot.ColumnDefinitions.Add(new ColumnDefinition { Width = Star });
        var baseline = Divider(new Thickness(0));
        baseline.VerticalAlignment = VerticalAlignment.Bottom;
        Place(baseline, columnSpan: n);
        plot.Children.Add(baseline);

        double gap = n > 20 ? 1.5 : 3;
        for (int i = 0; i < n; i++)
        {
            double v = r.Bars[i] * share;
            var slot = new Grid { Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent) };
            Place(slot, column: i);
            if (v > 0)
            {
                var bar = new Border
                {
                    Style = S("PanelChartBarStyle"),
                    Height = Math.Max(2, v / max * (plotHeight - 2)),
                    Margin = new Thickness(gap, 0, gap, 1),
                    MaxWidth = 22,
                };
                slot.Children.Add(bar);
                slot.PointerEntered += (_, _) => bar.Opacity = 0.75;
                slot.PointerExited += (_, _) => bar.Opacity = 1;
            }
            ToolTipService.SetToolTip(slot, $"{r.SlotLabels[i]}  {FakeData.Money(v)}");
            plot.Children.Add(slot);
        }
        ChartArea.Children.Add(plot);

        AxisArea.Children.Clear();
        AxisArea.ColumnDefinitions.Clear();
        for (int i = 0; i < n; i++) AxisArea.ColumnDefinitions.Add(new ColumnDefinition { Width = Star });
        foreach (var a in r.Axis)
        {
            int span = a.Align == AxisAlign.Center ? 1 : Math.Min(n, Math.Max(4, n / 4));
            int column = a.Align == AxisAlign.Right ? Math.Max(0, n - span) : a.Index;
            var t = Text(a.Text, "PanelCaptionTertiaryStyle");
            t.TextTrimming = TextTrimming.None;
            t.HorizontalAlignment = a.Align switch
            {
                AxisAlign.Center => HorizontalAlignment.Center,
                AxisAlign.Right => HorizontalAlignment.Right,
                _ => HorizontalAlignment.Left,
            };
            Place(t, column: column, columnSpan: Math.Min(span, n - column));
            AxisArea.Children.Add(t);
        }
    }

    /// <summary>使用分布: Settings-style rows (name, value, share, a bar under each).</summary>
    private void RenderDimension()
    {
        var d = FakeData.Dimensions[_dimensionIndex];
        var r = FakeData.Ranges[_rangeIndex];
        var today = FakeData.Ranges[0];
        double tokenScale = r.TokensM / today.TokensM * (CurrentSubscription?.UsageShare ?? 1.0);
        double costScale = r.Cost / today.Cost * (CurrentSubscription?.UsageShare ?? 1.0);
        bool byCost = _byCost && d.HasCost;
        MetricBox.IsEnabled = d.HasCost;

        double total = d.Items.Sum(i => byCost ? i.Cost : i.TokensM);
        DimensionTotal.Text = byCost
            ? $"合计 {FakeData.Money(total * costScale)}"
            : $"合计 {FakeData.Tokens(total * tokenScale)} Token";

        DimensionList.Children.Clear();
        foreach (var item in d.Items)
        {
            double raw = byCost ? item.Cost : item.TokensM;
            double pct = total > 0 ? raw / total * 100 : 0;
            var g = new Grid { RowSpacing = 6 };
            g.RowDefinitions.Add(new RowDefinition { Height = Auto });
            g.RowDefinitions.Add(new RowDefinition { Height = Auto });
            var top = Columns(Star, Auto, Px(44));
            var value = Text(byCost ? FakeData.Money(raw * costScale) : FakeData.Tokens(raw * tokenScale), "PanelBodyStyle", TextAlignment.Right);
            var share = Text($"{pct:0}%", "PanelCaptionSecondaryStyle", TextAlignment.Right);
            Place(value, column: 1);
            Place(share, column: 2);
            top.Children.Add(Text(item.Name, "PanelBodyStyle"));
            top.Children.Add(value);
            top.Children.Add(share);
            var bar = new ProgressBar { Style = S("PanelBarStyle"), Value = pct };
            AutomationProperties.SetName(bar, $"{item.Name} {pct:0}%");
            Place(bar, row: 1);
            g.Children.Add(top);
            g.Children.Add(bar);
            DimensionList.Children.Add(g);
        }
    }
}
