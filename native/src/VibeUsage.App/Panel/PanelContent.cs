using System;
using System.Collections.Generic;
using System.Linq;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Documents;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Shapes;
using VibeUsage.App.Shell;
using VibeUsage.Display;

namespace VibeUsage.App.Panel;

/// <summary>
/// What the panel draws, rebuilt from <see cref="State.AppModel"/> on every
/// change. The container is final: header (Mac <c>ModernHeaderBar</c>), the
/// 概览 row (<c>ModernSubscriptionRow</c>), the scrolling cards, the footer
/// (<c>ModernFooterBar</c>), sizes and type from the Mac theme. The card
/// bodies are placeholders fed by the display library: names, figures and
/// verdicts only; meters, rings, the curve, filters and the distribution
/// rings come with the quota card (#95) and recent usage (#96).
///
/// Every control is a real <see cref="Button"/> with a Narrator name, so
/// keyboard focus, Enter/Space and the Windows focus ring work from the start.
/// </summary>
internal sealed partial class PanelWindow
{
    private readonly Dictionary<string, Button> focusables = [];
    private string? focusKey;
    private bool focusByKeyboard;
    private string? lastSyncKind;

    private void Render()
    {
        var now = DateTimeOffset.Now;
        focusables.Clear();
        var quota = model.QuotaDisplay(now);
        headerHost.Children.Clear();
        headerHost.Children.Add(Header());
        rowHost.Children.Clear();
        // One subscription opens straight on it and none has nothing to choose: no 概览 row (132f, 132g).
        rowHost.Children.Add(quota.Buttons.Count > 1 ? SubscriptionRow(quota) : new Grid { Height = 4 });
        var usage = model.IsConfigured ? model.UsageDisplay(now) : null;
        cards.Children.Clear();
        cards.Children.Add(QuotaCard(quota));
        cards.Children.Add(UsageCard(usage));
        if (model.IsConfigured && model.Panel.DetailsExpanded) cards.Children.Add(Details(usage));
        RenderFooter();
    }

    private void RenderFooter()
    {
        footerHost.Children.Clear();
        footerHost.Children.Add(Footer());
        RestoreFocus();
    }

    /// <summary>After a rebuild, the control the user just used keeps the keyboard, if the panel has it.</summary>
    private void RestoreFocus()
    {
        if (focusKey is not string key || !focusables.TryGetValue(key, out var b)) return;
        if (Win32.GetForegroundWindow() != Hwnd) return;
        var state = focusByKeyboard ? FocusState.Keyboard : FocusState.Programmatic;
        DispatcherQueue.TryEnqueue(() => b.Focus(state));
    }

    // -- header (136a) ------------------------------------------------------------------------

    private UIElement Header()
    {
        var g = new Grid { Height = 40, Padding = new Thickness(Theme.TextInset, 12, Theme.TextInset, 8) };
        double baseline = Theme.BaselineIn(20, (13, Face.Text));
        g.Children.Add(Theme.AtBaseline(Theme.T("Vibe Usage", 13, Face.Text, Wt.SemiBold, Theme.Text, -0.13), baseline));

        var links = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 16, HorizontalAlignment = HorizontalAlignment.Right };
        links.Children.Add(WebLink("网页版", "web", "usage", baseline));
        links.Children.Add(WebLink("排行榜", "rank", "rank", baseline));
        var settings = Theme.T("设置", 13, Face.Text, Wt.Regular, Theme.Secondary);
        links.Children.Add(Bare(Sized(Theme.AtBaseline(settings, baseline), 20), "settings", "设置",
            () => SettingsRequested?.Invoke(), Lights(Theme.Secondary, b => settings.Foreground = b)));
        g.Children.Add(links);
        return g;
    }

    private Button WebLink(string title, string key, string path, double baseline)
    {
        var text = Theme.T(title, 13, Face.Text, Wt.Regular, Theme.Secondary);
        var arrowBrush = new SolidColorBrush(Theme.TextC(0.62 * 0.8));
        var arrow = Icons.ArrowUpRight(arrowBrush);
        arrow.VerticalAlignment = VerticalAlignment.Top;
        arrow.Margin = new Thickness(3, 7, 0, 0);
        var row = new StackPanel { Orientation = Orientation.Horizontal };
        row.Children.Add(Theme.AtBaseline(text, baseline));
        row.Children.Add(arrow);
        return Bare(Sized(row, 20), key, title, () => model.OpenWeb(path), on =>
        {
            text.Foreground = on ? Theme.Text : Theme.Secondary;
            foreach (var p in arrow.Children.OfType<Path>()) p.Stroke = on ? new SolidColorBrush(Theme.TextC(0.8)) : arrowBrush;
        });
    }

    // -- 概览 row (133) -----------------------------------------------------------------------

    private UIElement SubscriptionRow(QuotaPanelDisplay quota)
    {
        bool overview = quota.Detail == null;
        var g = new Grid { Height = 46, Padding = new Thickness(Theme.TextInset, 0, Theme.TextInset, 12) };
        var row = new StackPanel { Orientation = Orientation.Horizontal, VerticalAlignment = VerticalAlignment.Center, Height = 34 };

        var label = Theme.T("概览", 13, Face.Text, overview ? Wt.SemiBold : Wt.Medium, overview ? Theme.Text : Theme.Secondary);
        var overviewContent = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 7 };
        overviewContent.Children.Add(Icons.Lamp(overview));
        overviewContent.Children.Add(Theme.AtBaseline(label, Theme.BaselineIn(34, (13, Face.Text))));
        row.Children.Add(Bare(Sized(overviewContent, 34), "overview", "概览", () => model.Select(null)));

        var sep = VRule(16);
        sep.Margin = new Thickness(12, 0, 12, 0);
        row.Children.Add(sep);

        var buttons = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6, VerticalAlignment = VerticalAlignment.Center };
        foreach (var s in quota.Buttons)
        {
            bool selected = quota.Detail?.Provider == s.Provider;
            var holder = new Grid { Width = 30, Height = 30 };
            holder.Children.Add(Icons.RoundButtonFace(s.Provider, selected, false));
            var provider = s.Provider;
            buttons.Children.Add(Bare(holder, "sub-" + provider, s.ButtonHelp, () => model.Select(provider), on =>
            {
                holder.Children.Clear();
                holder.Children.Add(Icons.RoundButtonFace(provider, selected, on));
            }, s.ButtonHelp));
        }
        row.Children.Add(buttons);
        g.Children.Add(row);

        var gear = Icons.Fluent(Icons.Gear, 12.5, Theme.Tertiary);
        var gearBox = new Grid { Width = 22, Height = 22, Background = Theme.Transparent };
        gearBox.Children.Add(gear);
        var gearButton = Bare(gearBox, "gear", "打开订阅配额设置", () => SettingsRequested?.Invoke(),
            Lights(Theme.Tertiary, b => gear.Foreground = b), "打开订阅配额设置");
        gearButton.HorizontalAlignment = HorizontalAlignment.Right;
        gearButton.Margin = new Thickness(0, 0, -4, 0);
        g.Children.Add(gearButton);
        return g;
    }

    // -- cards ----------------------------------------------------------------------------------

    /// <summary>Mac <c>ModernCard</c>: 20 DIP title row (13 semibold, secondary), the body 8 below.</summary>
    private static Grid CardFrame(string title, UIElement body)
    {
        var stack = new StackPanel { Spacing = 8 };
        var titleRow = new Grid { Height = 20 };
        titleRow.Children.Add(Theme.AtBaseline(Theme.T(title, 13, Face.Text, Wt.SemiBold, Theme.Secondary), Theme.BaselineIn(20, (13, Face.Text))));
        stack.Children.Add(titleRow);
        stack.Children.Add(body);
        return Theme.Card(stack);
    }

    private static Grid Body(double height, params UIElement[] children)
    {
        var g = new Grid { Height = height };
        foreach (var c in children) g.Children.Add(c);
        return g;
    }

    // 订阅配额 (placeholder body) ----------------------------------------------------------------

    private UIElement QuotaCard(QuotaPanelDisplay quota)
    {
        UIElement body = quota.IsEmpty ? QuotaEmpty(quota)
            : quota.Detail is { } detail ? QuotaDetail(detail)
            : QuotaOverview(quota.Overview);
        return CardFrame("订阅配额", Body(Theme.CardBodyHeight, body));
    }

    private UIElement QuotaEmpty(QuotaPanelDisplay quota)
    {
        var stack = new StackPanel { Spacing = 10, HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center };
        var title = Theme.T(quota.EmptyTitle ?? "", 13, Face.Text, Wt.Regular, Theme.Secondary);
        title.HorizontalAlignment = HorizontalAlignment.Center;
        stack.Children.Add(title);
        var action = Theme.T(quota.EmptyActionLabel ?? "", 13, Face.Text, Wt.Medium, Theme.Text);
        var button = Bare(action, "empty-settings", quota.EmptyActionLabel ?? "", () => SettingsRequested?.Invoke());
        button.HorizontalAlignment = HorizontalAlignment.Center;
        stack.Children.Add(button);
        return stack;
    }

    /// <summary>One line per subscription: mark and name, its windows' remaining share, the verdict.</summary>
    private static UIElement QuotaOverview(QuotaOverviewDisplay overview)
    {
        var stack = new StackPanel();
        foreach (var s in overview.Rows) stack.Children.Add(QuotaLine(s));
        if (overview.Remainder is { } rest)
        {
            var text = Theme.Block(Theme.R(rest.Label, 11), Theme.R(rest.Summary is { } sum ? " · " + sum : "", 11));
            text.Foreground = Toned(rest.Tone, Theme.Secondary);
            stack.Children.Add(Theme.Line(text, 30));
        }
        return stack;
    }

    private static UIElement QuotaLine(QuotaSubscriptionDisplay s)
    {
        var g = new Grid { Height = 34 };
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(80) });

        var name = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8, VerticalAlignment = VerticalAlignment.Center };
        name.Children.Add(Icons.Mark(s.Provider, 16, Theme.Strong));
        var title = Theme.T(s.Provider.OverviewName(), 13, Face.Text, Wt.Medium, Theme.Text);
        name.Children.Add(title);
        g.Children.Add(name);

        var chips = Theme.Block();
        foreach (var chip in s.OverviewChips)
        {
            if (chips.Inlines.Count > 0) chips.Inlines.Add(Theme.Gap(12));
            chips.Inlines.Add(Theme.R(chip.Label, 11, Face.Mono, Wt.Regular, Theme.Secondary));
            chips.Inlines.Add(Theme.Gap(5));
            chips.Inlines.Add(Theme.R(QuotaPercent(chip.Remaining), 13, Face.Mono, Wt.Medium, Toned(chip.Tone, Theme.Text)));
            chips.Inlines.Add(Theme.R("%", 11, Face.Mono, Wt.Regular, Theme.Secondary));
        }
        if (s.OverviewChips.Count == 0)
            chips.Inlines.Add(Theme.R(s.ButtonStatus, 11, Face.Text, Wt.Regular, Theme.Secondary));
        Grid.SetColumn(chips, 1);
        g.Children.Add(chips);

        var comment = Theme.T(s.Comment ?? "", 11, Face.Text, Wt.Regular, Toned(s.CommentTone, Theme.Secondary));
        comment.HorizontalAlignment = HorizontalAlignment.Right;
        Grid.SetColumn(comment, 2);
        g.Children.Add(comment);
        return g;
    }

    /// <summary>A subscription on its own: its plan and up to three windows, each with the forecast.</summary>
    private static UIElement QuotaDetail(QuotaSubscriptionDisplay d)
    {
        var stack = new StackPanel();
        var head = new Grid { Height = 30 };
        var name = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8, VerticalAlignment = VerticalAlignment.Center };
        name.Children.Add(Icons.Mark(d.Provider, 16, Theme.Strong));
        var title = Theme.Block(Theme.R(d.Provider.DisplayName(), 13, Face.Text, Wt.SemiBold),
            Theme.R(d.PlanLabel is { } plan ? "  " + plan : "", 11, Face.Text, Wt.Regular, Theme.Secondary));
        name.Children.Add(title);
        head.Children.Add(name);
        var comment = Theme.T(d.Comment ?? "", 11, Face.Text, Wt.Regular, Toned(d.CommentTone, Theme.Secondary));
        comment.HorizontalAlignment = HorizontalAlignment.Right;
        head.Children.Add(comment);
        stack.Children.Add(head);

        if (d.CompactRows.Count == 0)
        {
            stack.Children.Add(Theme.Line(Theme.T(d.DetailTitle ?? d.ButtonStatus, 13, Face.Text, Wt.Regular, Theme.Text), 30));
            if (d.DetailReason is { } reason) stack.Children.Add(Theme.Line(Theme.T(reason, 11, Face.Text, Wt.Regular, Theme.Secondary), 20));
            return stack;
        }
        foreach (var m in d.CompactRows)
        {
            var line = new Grid { Height = 30 };
            var figure = Theme.Block(Theme.R(m.Label, 11, Face.Mono, Wt.Regular, Theme.Secondary), Theme.Gap(8),
                Theme.R(m.Remaining is double r ? QuotaPercent(r) : "—", 13, Face.Mono, Wt.Medium, Toned(m.Tone, Theme.Text)),
                Theme.R(m.Remaining is null ? "" : "%", 11, Face.Mono, Wt.Regular, Theme.Secondary));
            line.Children.Add(figure);
            var note = Theme.T(m.Prediction?.Text ?? m.Message ?? "", 11, Face.Text, Wt.Regular, Theme.Secondary);
            note.HorizontalAlignment = HorizontalAlignment.Right;
            line.Children.Add(note);
            stack.Children.Add(line);
        }
        if (d.FoldLabel is { } fold) stack.Children.Add(Theme.Line(Theme.T(fold, 11, Face.Text, Wt.Regular, Theme.Secondary), 24));
        return stack;
    }

    private static string QuotaPercent(double remaining) => QuotaFormat.Percent(remaining);

    private static Brush Toned(PaceTone tone, Brush neutral) => tone switch
    {
        PaceTone.Red => Theme.RedBrush,
        PaceTone.Amber => Theme.AmberBrush,
        _ => neutral,
    };

    // 近期消耗 (placeholder body) ----------------------------------------------------------------

    private UIElement UsageCard(UsageDisplaySnapshot? usage)
    {
        var column = new Grid { Width = Theme.LeadingColumnWidth, HorizontalAlignment = HorizontalAlignment.Left };
        if (model.IsConfigured)
        {
            if (usage == null)
            {
                column.Children.Add(Placed(Placeholder(96, 30), 0, 7));
            }
            else
            {
                var parts = RecentUsageNumber.Parts(RecentUsageNumber.Cost(usage.Total.Cost));
                // CSS line-height 1: a 44 DIP box; JetBrains Mono's baseline sits 37.8 DIP down it (Mac RecentUsageBigNumber).
                column.Children.Add(Theme.AtBaseline(BigNumber(parts), 37.8));
                column.Children.Add(Placed(Theme.AtBaseline(Theme.T(RecentUsageFigures.Label(Hero.Cost), 11, Face.Text, Wt.Regular, Theme.Secondary),
                    Theme.BaselineIn(18, (11, Face.Text))), 0, 44));
                column.Children.Add(Placed(FigureRow("Token", RecentUsageNumber.Tokens(usage.Total.TotalTokens)), 0, Theme.CardBodyHeight - 18));
            }
        }
        return CardFrame("近期消耗", Body(Theme.CardBodyHeight, column));
    }

    /// <summary>Mac <c>RecentUsageBigNumber</c>: 44 DIP light integer, the symbol and cents at 22.</summary>
    private static TextBlock BigNumber(IReadOnlyList<NumberPart> parts)
    {
        var tb = Theme.Block();
        foreach (var part in parts)
        {
            switch (part.Role)
            {
                case NumberRole.Symbol:
                    tb.Inlines.Add(Theme.R(part.Text, 22, Face.Mono, Wt.Light, Theme.Tertiary));
                    tb.Inlines.Add(Theme.Gap(1));
                    break;
                case NumberRole.Integer:
                    tb.Inlines.Add(Theme.R(part.Text, 44, Face.Mono, Wt.Light, Theme.Text, -2.64));
                    break;
                case NumberRole.Fraction:
                    tb.Inlines.Add(Theme.R("." + part.Text, 22, Face.Mono, Wt.Light, Theme.Secondary, -1.1));
                    break;
                default:
                    tb.Inlines.Add(Theme.Gap(2));
                    tb.Inlines.Add(Theme.R(part.Text, 22, Face.Mono, Wt.Light, Theme.Secondary));
                    break;
            }
        }
        return tb;
    }

    /// <summary>Mac <c>RecentUsageRowLabel</c>: 11 DIP label in a 36 DIP column, the value at 13 medium.</summary>
    private static Grid FigureRow(string label, string value)
    {
        var row = new Grid { Height = 18 };
        double baseline = Theme.BaselineIn(18, (11, Face.Text), (13, Face.Mono));
        row.Children.Add(Theme.AtBaseline(Theme.T(label, 11, Face.Text, Wt.Regular, Theme.Secondary), baseline));
        var v = Theme.AtBaseline(Theme.T(value, 13, Face.Mono, Wt.Medium, Theme.Text), baseline);
        v.Margin = new Thickness(46, 0, 0, 0);
        row.Children.Add(v);
        return row;
    }

    private static Border Placeholder(double width, double height) => new()
    {
        Width = width,
        Height = height,
        CornerRadius = new CornerRadius(3),
        Background = Theme.Placeholder,
    };

    // 展开详情 (placeholder bodies) --------------------------------------------------------------

    private static UIElement Details(UsageDisplaySnapshot? usage)
    {
        var g = new Grid { ColumnSpacing = Theme.CardSpacing, RowSpacing = Theme.CardSpacing };
        g.ColumnDefinitions.Add(new ColumnDefinition());
        g.ColumnDefinitions.Add(new ColumnDefinition());
        g.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        g.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        (string Title, UsageRanking? Ranking)[] cards =
        [
            ("模型分布", usage?.Ranking(Dimension.Model, Metric.Tokens)),
            ("Token 构成", usage?.TokenComposition),
            ("终端分布", usage?.Ranking(Dimension.Terminal, Metric.Tokens)),
            ("工具分布", usage?.Ranking(Dimension.Tool, Metric.Tokens)),
        ];
        for (int i = 0; i < cards.Length; i++)
        {
            // Mac ModernDetailCard: 252 DIP tall.
            var card = CardFrame(cards[i].Title, Body(252 - 2 * Theme.CardPadding - 20 - 8, RankingRows(cards[i].Ranking)));
            Grid.SetRow(card, i / 2);
            Grid.SetColumn(card, i % 2);
            g.Children.Add(card);
        }
        return g;
    }

    private static UIElement RankingRows(UsageRanking? ranking)
    {
        var stack = new StackPanel { Margin = new Thickness(0, 4, 0, 0) };
        if (ranking == null) return stack;
        foreach (var row in ranking.Listed)
        {
            var line = new Grid { Height = 22 };
            line.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(14) });
            line.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            line.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            line.Children.Add(new Ellipse { Width = 6, Height = 6, Fill = new SolidColorBrush(RankColor(row.Color)), VerticalAlignment = VerticalAlignment.Center });
            var name = Theme.T(row.Name, 11, Face.Text, Wt.Regular, Theme.Strong);
            name.TextTrimming = TextTrimming.CharacterEllipsis;
            Grid.SetColumn(name, 1);
            line.Children.Add(name);
            var value = Theme.T(RecentUsageNumber.Tokens((long)row.Value), 11, Face.Mono, Wt.Regular, Theme.Secondary);
            Grid.SetColumn(value, 2);
            line.Children.Add(value);
            stack.Children.Add(line);
        }
        if (ranking.RemainderLabel is { } rest) stack.Children.Add(Theme.Line(Theme.T(rest, 11, Face.Text, Wt.Regular, Theme.Secondary), 22));
        return stack;
    }

    private static Windows.UI.Color RankColor(Display.RankColor color) => color switch
    {
        Display.RankColor.Blue => Theme.RankBlue,
        Display.RankColor.Teal => Theme.RankTeal,
        Display.RankColor.Magenta => Theme.RankMagenta,
        _ => Theme.RankGray,
    };

    // -- footer (136c–136j) -----------------------------------------------------------------------

    private UIElement Footer()
    {
        var g = new Grid { Height = 38, Padding = new Thickness(Theme.TextInset, 10, Theme.TextInset, 10) };
        var right = new StackPanel { Orientation = Orientation.Horizontal, HorizontalAlignment = HorizontalAlignment.Right };

        if (model.IsConfigured)
        {
            bool expanded = model.Panel.DetailsExpanded;
            string detailsText = expanded ? "收起详情" : "展开详情";
            var label = Theme.T(detailsText, 13, Face.Text, Wt.Regular, Theme.Text);
            var chevron = Icons.Chevron(expanded, new SolidColorBrush(Theme.TextC(0.7)));
            chevron.VerticalAlignment = VerticalAlignment.Top;
            chevron.Margin = new Thickness(8, 9 - chevron.Height / 2, 0, 0);
            var left = new StackPanel { Orientation = Orientation.Horizontal };
            left.Children.Add(Theme.AtBaseline(label, Theme.BaselineIn(18, (13, Face.Text))));
            left.Children.Add(chevron);
            var details = Bare(Sized(left, 18), "details", detailsText, model.ToggleDetails);
            details.HorizontalAlignment = HorizontalAlignment.Left;
            g.Children.Add(details);

            right.Children.Add(SyncButton());
            var sep = VRule(12);
            sep.Margin = new Thickness(10, 0, 10, 0);
            right.Children.Add(sep);
        }

        var power = Icons.Fluent(Icons.Power, 10, Theme.Secondary);
        var powerBox = new Grid { Width = 12, Height = 18, Background = Theme.Transparent };
        powerBox.Children.Add(power);
        right.Children.Add(Bare(powerBox, "quit", "退出 Vibe Usage", () => QuitRequested?.Invoke(),
            Lights(Theme.Secondary, b => power.Foreground = b), "退出 Vibe Usage"));
        g.Children.Add(right);
        return g;
    }

    /// <summary>「同步用量」: usage only, never quotas (Mac <c>ModernSyncButton</c>). Narrator hears its state change.</summary>
    private Button SyncButton()
    {
        var label = SyncLabel.Make(model.Sync.Status, model.Sync.Message, model.LastSuccessfulSync, DateTimeOffset.Now);
        var content = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6 };
        double baseline = Theme.BaselineIn(18, (11, Face.Mono), (11, Face.Text));
        TextBlock text;
        string spoken;
        string help = "同步用量";
        Action<bool>? hover = null;
        switch (label)
        {
            case SyncLabel.Syncing:
                content.Children.Add(Icons.Spinner(12));
                text = Theme.T("同步中", 11, Face.Text, Wt.Regular, Theme.Secondary);
                spoken = "同步中";
                break;
            case SyncLabel.Failed failed:
                content.Children.Add(Icons.Alert(Theme.RedBrush));
                text = Theme.Block(Theme.R("同步出错", 11, Face.Text, Wt.Regular, Theme.Strong));
                if (failed.Reason.Length > 0) text.Inlines.Add(Theme.R("  " + failed.Reason, 11, Face.Text, Wt.Regular, Theme.Secondary));
                text.Inlines.Add(Theme.R("  重试", 11, Face.Text, Wt.Medium, Theme.Text));
                text.MaxWidth = 300;
                text.TextTrimming = TextTrimming.CharacterEllipsis;
                spoken = failed.Reason.Length > 0 ? $"同步出错，{failed.Reason}，重试" : "同步出错，重试";
                help = failed.Detail;
                break;
            default:
            {
                var icon = Icons.Sort(Theme.Secondary);
                content.Children.Add(icon);
                text = label is SyncLabel.Synced synced
                    ? Theme.Block(Theme.R(synced.Number ?? "", 11, Face.Mono), Theme.R(synced.Text, 11))
                    : Theme.Block(Theme.R("还没同步过", 11));
                text.Foreground = Theme.Secondary;
                spoken = label is SyncLabel.Synced s ? (s.Number ?? "") + s.Text : "还没同步过";
                var t = text;
                hover = Lights(Theme.Secondary, b => t.Foreground = b, b => { foreach (var p in icon.Children.OfType<Path>()) p.Stroke = b; });
                break;
            }
        }
        content.Children.Add(Theme.AtBaseline(text, baseline));
        var button = Bare(Sized(content, 18), "sync", "同步用量，" + spoken, model.TriggerSync, hover, help);
        button.IsEnabled = label is not SyncLabel.Syncing;
        AutomationProperties.SetLiveSetting(button, AutomationLiveSetting.Polite);

        string kind = label.GetType().Name;
        if (lastSyncKind != null && lastSyncKind != kind && AutomationPeer.ListenerExists(AutomationEvents.LiveRegionChanged))
        {
            DispatcherQueue.TryEnqueue(() =>
                FrameworkElementAutomationPeer.CreatePeerForElement(button)?.RaiseAutomationEvent(AutomationEvents.LiveRegionChanged));
        }
        if (lastSyncKind != kind) ShellLog.Write($"sync label {kind}");
        lastSyncKind = kind;
        return button;
    }

    // -- small parts ------------------------------------------------------------------------------

    /// <summary>
    /// A chrome-less button around drawn content: keyboard focus, Enter/Space
    /// and Narrator as a normal Windows button; the pointer brightens it the
    /// Mac way (<paramref name="hover"/>).
    /// </summary>
    private Button Bare(UIElement content, string key, string name, Action click, Action<bool>? hover = null, string? tooltip = null)
    {
        var b = new Button
        {
            Style = (Style)Application.Current.Resources["BareButtonStyle"],
            Content = content,
            HorizontalAlignment = HorizontalAlignment.Left,
            VerticalAlignment = VerticalAlignment.Center,
            // A click must not move keyboard focus: that would activate the panel
            // opened from the tray and flip the input method's 中/英. Tab still reaches it.
            AllowFocusOnInteraction = false,
        };
        AutomationProperties.SetName(b, name);
        if (tooltip != null)
        {
            var tip = new ToolTip { Content = tooltip };
            tip.Opened += (_, _) => Note("tooltip", key);
            ToolTipService.SetToolTip(b, tip);
        }
        b.Click += (_, _) =>
        {
            focusKey = key;
            focusByKeyboard = b.FocusState == FocusState.Keyboard;
            ShellLog.Write($"click {key}");
            click();
        };
        b.PointerEntered += (_, _) =>
        {
            Note("hover", key);
            hover?.Invoke(true);
        };
        if (hover != null)
        {
            b.PointerExited += (_, _) => hover(false);
            b.PointerCanceled += (_, _) => hover(false);
        }
        focusables[key] = b;
        return b;
    }

    /// <summary>Text and icons that light to the primary colour under the pointer (Mac <c>ModernTextButtonStyle</c>).</summary>
    private static Action<bool> Lights(Brush rest, params Action<Brush>[] setters) => on =>
    {
        foreach (var set in setters) set(on ? Theme.Text : rest);
    };

    private static Grid Sized(UIElement child, double height)
    {
        var g = new Grid { Height = height, Background = Theme.Transparent };
        g.Children.Add(child);
        return g;
    }

    private static Rectangle VRule(double height) => new()
    {
        Width = 1,
        Height = height,
        Fill = Theme.Separator,
        VerticalAlignment = VerticalAlignment.Center,
    };

    private static FrameworkElement Placed(FrameworkElement e, double left, double top)
    {
        e.HorizontalAlignment = HorizontalAlignment.Left;
        e.VerticalAlignment = VerticalAlignment.Top;
        e.Margin = new Thickness(left, top, 0, 0);
        return e;
    }
}
