using System;
using System.Collections.Generic;
using System.Linq;
using Microsoft.UI.Input;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Shapes;
using VibeUsage.App.Shell;
using VibeUsage.Display;
using Windows.Foundation;

namespace VibeUsage.App.Panel;

/// <summary>
/// The 订阅配额 card (Mac <c>ModernQuotaCard.swift</c>, design 131–133): the
/// overview of every enabled subscription, one subscription's detail, the
/// empty state, 「刷新额度」 and the round buttons of the 概览 row. What to
/// draw — counts, tones, comments, the tightest allowance, the rings, the
/// row order, every state's words — comes from <see cref="QuotaPanelDisplay"/>;
/// this file lays it out at the Mac's sizes and formats numbers.
/// Hover, the unfolded list and dragging are view state.
/// </summary>
internal sealed partial class PanelWindow
{
    private Provider? hoveredOverviewRow;
    private string? hoveredDetailRow;
    private bool quotaExpanded;
    private Provider? quotaShownDetail;
    private readonly Canvas overlay = new() { IsHitTestVisible = false };
    private (FrameworkElement Row, MeterTooltip Tip)? tooltip;
    private FrameworkElement? tooltipBlock;
    private Canvas? quotaUnderlay;
    private Border? hoverBand;

    // -- card -----------------------------------------------------------------------------------

    private UIElement QuotaCard(QuotaPanelDisplay quota)
    {
        if (quota.Detail?.Provider != quotaShownDetail)
        {
            // A new subscription starts folded, without a hovered row (Mac: .id(detail.provider)).
            quotaShownDetail = quota.Detail?.Provider;
            quotaExpanded = false;
            hoveredDetailRow = null;
        }
        tooltip = null;
        hoverBand = null;
        quotaUnderlay = new Canvas { IsHitTestVisible = false };
        UIElement body = quota.IsEmpty ? QuotaEmpty()
            : quota.Detail is { } detail ? QuotaDetail(detail, quota)
            : QuotaOverview(quota.Overview);
        return CardFrame("订阅配额", body, quota.IsEmpty ? null : RefreshButton(quota), quotaUnderlay);
    }

    /// <summary>
    /// 「刷新额度」 (132s–132u): quotas only. It spins and takes no clicks while
    /// a read in its scope is in flight, including the reads an open starts.
    /// </summary>
    private Button RefreshButton(QuotaPanelDisplay quota)
    {
        bool spinning = quota.IsRefreshing;
        var box = new Grid { Width = 14, Height = 14, Background = Theme.Transparent };
        Canvas? icon = null;
        if (spinning)
        {
            box.Children.Add(Icons.Spinner(13));
        }
        else
        {
            icon = QuotaIcons.Refresh(Theme.Secondary);
            box.Children.Add(icon);
        }
        var b = Bare(box, "refresh", "刷新额度", () => model.RefreshQuotas(quota),
            icon == null ? null : Lights(Theme.Secondary, brush => QuotaIcons.Paint(icon, brush)), "刷新额度");
        b.HorizontalAlignment = HorizontalAlignment.Right;
        b.IsEnabled = !spinning;
        return b;
    }

    // -- empty (132f) ---------------------------------------------------------------------------

    private UIElement QuotaEmpty()
    {
        var g = new Grid { Height = Theme.CardBodyHeight };
        var zero = Theme.Block(Theme.R("0", 44, Face.Mono, Wt.Light, null, -2.64));
        zero.Foreground = Theme.Secondary;
        g.Children.Add(Theme.At(Theme.AtBaseline(zero, BigBaseline), -2, 0));
        g.Children.Add(Theme.At(Theme.AtBaseline(Theme.T("个订阅", 11, Face.Text, Wt.Regular, Theme.Secondary),
            44 + Theme.BaselineIn(18, (11, Face.Text))), 0, 0));

        // Only the title and the way to Settings; no line explaining what the card will show.
        g.Children.Add(Theme.At(Theme.AtBaseline(Theme.T("还没有要看的订阅", 13, Face.Text, Wt.Medium, Theme.Text),
            3 + Theme.BaselineIn(18, (13, Face.Text))), Theme.LeadingColumnWidth, 0));
        var label = Theme.T("去设置", 13, Face.Text, Wt.Regular, Theme.Text);
        var content = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 7, Height = Theme.LineHeight(13, Face.Text) };
        content.Children.Add(Sized(Theme.AtBaseline(label, Theme.Ascent(13, Face.Text)), Theme.LineHeight(13, Face.Text)));
        var chevron = QuotaIcons.ChevronRight(Theme.Solid(Theme.TextC(0.7)));
        content.Children.Add(chevron);
        var go = Bare(content, "empty-settings", "去设置", () => SettingsRequested?.Invoke());
        g.Children.Add(Theme.At(go, Theme.LeadingColumnWidth, 3 + 18 + 12));
        return g;
    }

    /// <summary>The 44 DIP light number's baseline in its 44 DIP line (CSS line-height 1).</summary>
    private static double BigBaseline => Theme.BaselineIn(44, (44, Face.Mono));

    // -- overview (131a, 132a–132e, 132h) -------------------------------------------------------

    private UIElement QuotaOverview(QuotaOverviewDisplay overview)
    {
        var g = new Grid { Height = Theme.CardBodyHeight };

        var count = Theme.Block(
            Theme.R(overview.AttentionCount.ToString(System.Globalization.CultureInfo.InvariantCulture), 44, Face.Mono, Wt.Light,
                Toned(overview.Tone, Theme.Text), -2.64),
            Theme.Gap(2),
            Theme.R("/" + overview.Total.ToString(System.Globalization.CultureInfo.InvariantCulture), 22, Face.Mono, Wt.Light,
                Theme.Secondary, -0.88));
        g.Children.Add(Theme.At(Theme.AtBaseline(count, BigBaseline), -2, 0));
        g.Children.Add(Theme.At(Theme.AtBaseline(Theme.T("个订阅要留意", 11, Face.Text, Wt.Regular, Theme.Secondary),
            44 + Theme.BaselineIn(18, (11, Face.Text))), 0, 0));
        g.Children.Add(Theme.At(CountRow("快用完", overview.NearlyEmpty, PaceTone.Red), 0, Theme.CardBodyHeight - 36));
        g.Children.Add(Theme.At(CountRow("偏快", overview.Fast, PaceTone.Amber), 0, Theme.CardBodyHeight - 18));

        var rows = new Grid { Width = Theme.RightColumnWidth, Height = Theme.CardBodyHeight };
        double y = 0;
        foreach (var row in overview.Rows)
        {
            rows.Children.Add(Theme.At(OverviewRow(row, overview.RowPitch), 0, y));
            y += overview.RowPitch;
        }
        if (overview.Remainder is { } rest) rows.Children.Add(Theme.At(Remainder(rest), 0, Theme.CardBodyHeight - 18 - 0.5));
        g.Children.Add(Theme.At(rows, Theme.LeadingColumnWidth, 0));
        return g;
    }

    /// <summary>「快用完 1」: label in a 36 DIP column, the count coloured when above zero.</summary>
    private static Grid CountRow(string label, int value, PaceTone tone)
    {
        var row = new Grid { Height = 18, Width = Theme.LeadingColumnWidth };
        double baseline = Theme.BaselineIn(18, (11, Face.Text), (13, Face.Mono));
        row.Children.Add(Theme.AtBaseline(Theme.T(label, 11, Face.Text, Wt.Regular, Theme.Secondary), baseline));
        var v = Theme.T(value.ToString(System.Globalization.CultureInfo.InvariantCulture), 13, Face.Mono, Wt.Medium,
            value > 0 ? Toned(tone, Theme.Text) : Theme.Secondary);
        row.Children.Add(Theme.At(Theme.AtBaseline(v, baseline), 46, 0));
        return row;
    }

    /// <summary>
    /// One subscription on the overview: mark, name, the percentages of its
    /// ring windows (plus an alerting allowance without a bar, 「Fable 9%」),
    /// the bar and the comment at the right. Clicking opens the subscription.
    /// </summary>
    private Button OverviewRow(QuotaSubscriptionDisplay row, double pitch)
    {
        const double width = Theme.RightColumnWidth;
        var provider = row.Provider;
        var g = new Grid { Width = width, Height = pitch, Background = Theme.Transparent };
        if (hoveredOverviewRow == provider) g.SizeChanged += (_, _) => { if (hoveredOverviewRow == provider) ShowBand(g, pitch - 1, 7, 0.06); };

        bool skeleton = row.ShowsSkeleton;
        bool nonSubscription = row.ReadState == ReadState.NonSubscriptionLogin;
        // 242 DIP, shorter only when a long state word would run into the bar.
        double barWidth = row.Comment is { } said && !skeleton && !nonSubscription
            ? Math.Min(Gauges.OverviewBarWidth, width - Theme.ApproximateWidth(said, 11) - 6)
            : Gauges.OverviewBarWidth;

        // The name line: 18 DIP, 1.5 DIP low, its capitals on the big number's top line.
        double baseline = 1 + 1.5 + Theme.BaselineIn(18, (13, Face.Text), (13, Face.Mono), (11, Face.Mono));
        g.Children.Add(Theme.At(QuotaIcons.Glyph(provider, 14, Theme.Strong), 0, baseline - 4.5 - 7));
        var name = Theme.T(provider.OverviewName(), 13, Face.Text, Wt.Medium, Theme.Text);
        g.Children.Add(Theme.At(Theme.AtBaseline(name, baseline), 20, 0));

        var meters = new Grid { Width = barWidth, HorizontalAlignment = HorizontalAlignment.Left, VerticalAlignment = VerticalAlignment.Top };
        if (skeleton)
        {
            meters.Children.Add(new Border
            {
                Width = 50, Height = 10, CornerRadius = new CornerRadius(3), Background = Theme.Placeholder,
                HorizontalAlignment = HorizontalAlignment.Right, VerticalAlignment = VerticalAlignment.Top,
                Margin = new Thickness(0, baseline - 10, 0, 0),
            });
        }
        else
        {
            var chips = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8, HorizontalAlignment = HorizontalAlignment.Right };
            foreach (var chip in row.OverviewChips)
            {
                var tb = Chip(chip.Label, chip.Remaining, chip.Tone);
                var holder = new Grid { VerticalAlignment = VerticalAlignment.Top };
                if (chip.IsColumn) holder.Width = 50;
                holder.Children.Add(Theme.AtBaseline(tb, baseline, HorizontalAlignment.Right));
                chips.Children.Add(holder);
            }
            meters.Children.Add(chips);
        }
        g.Children.Add(meters);

        const double lower = 1 + 18 - 1;
        if (skeleton)
        {
            g.Children.Add(Theme.At(new Border { Width = barWidth, Height = 5, CornerRadius = new CornerRadius(3), Background = Theme.Placeholder }, 0, lower + 5));
        }
        else if (nonSubscription && row.Comment is { } reason)
        {
            // 132e: the reason takes the bar's place; nothing at the right.
            var text = Theme.T(reason, 11, Face.Text, Wt.Regular, Theme.Secondary);
            g.Children.Add(Theme.At(Theme.AtBaseline(text, lower + 2 + Theme.BaselineIn(14, (11, Face.Text))), 0, 0));
        }
        else
        {
            g.Children.Add(Theme.At(Gauges.OverviewBar(Gauge.Of(row.Outer), Gauge.Of(row.Inner), barWidth), 0, lower));
        }

        if (!skeleton && !nonSubscription && row.Comment is { } comment)
        {
            // 「需登录」 and 「登录已失效」 ask the user to act: a step brighter than the other short states.
            var neutral = row.ReadState is ReadState.NeedsLogin or ReadState.LoginExpired ? Theme.Strong : Theme.Secondary;
            var tb = CommentText(comment, Toned(row.CommentTone, neutral), out bool mixed);
            double cb = 1 + 17 + (mixed ? Theme.BaselineIn(14, (11, Face.Text), (11, Face.Mono)) : Theme.BaselineIn(14, (11, Face.Text)));
            g.Children.Add(Theme.AtBaseline(tb, cb, HorizontalAlignment.Right));
        }

        string spoken = (provider.DisplayName() + " " + (row.Comment ?? "")).Trim();
        return Bare(g, "row-" + provider, spoken, () => model.Select(provider), on =>
        {
            if (on)
            {
                hoveredOverviewRow = provider;
                ShowBand(g, pitch - 1, 7, 0.06);
            }
            else if (hoveredOverviewRow == provider)
            {
                hoveredOverviewRow = null;
                HideBand();
            }
        });
    }

    /// <summary>
    /// The highlight under a hovered row, reaching 8 DIP past it on both
    /// sides (Mac: the row's background with a negative padding). It is drawn
    /// on a layer behind the whole card interior and placed from the row, so
    /// nothing between the card and the row clips it to the row's width.
    /// </summary>
    private void ShowBand(FrameworkElement row, double height, double radius, double alpha)
    {
        if (quotaUnderlay is not { } layer || row.ActualWidth <= 0) return;
        Point at;
        try
        {
            at = row.TransformToVisual(layer).TransformPoint(new Point(0, 0));
        }
        catch (ArgumentException)
        {
            return;
        }
        hoverBand ??= new Border();
        hoverBand.Width = row.ActualWidth + 16;
        hoverBand.Height = height;
        hoverBand.CornerRadius = new CornerRadius(radius);
        hoverBand.Background = Theme.Solid(Theme.TextC(alpha));
        Canvas.SetLeft(hoverBand, at.X - 8);
        Canvas.SetTop(hoverBand, at.Y);
        if (hoverBand.Parent == null) layer.Children.Add(hoverBand);
    }

    private void HideBand()
    {
        if (hoverBand?.Parent is Canvas layer) layer.Children.Remove(hoverBand);
    }

    /// <summary>「5h 12%」: label 11, number 13 medium, the percent sign dimmer unless the number is coloured (Mac ModernMeterChip).</summary>
    private static TextBlock Chip(string label, double remaining, PaceTone tone) => Theme.Block(
        Theme.R(label, 11, Face.Mono, Wt.Regular, Theme.Secondary),
        Theme.Gap(4),
        Theme.R(QuotaFormat.Percent(remaining), 13, Face.Mono, Wt.Medium, Toned(tone, Theme.Text), -0.26),
        Theme.R("%", 13, Face.Mono, Wt.Regular, tone == PaceTone.Neutral ? Theme.Tertiary : Toned(tone, Theme.Text)));

    /// <summary>「超额 17%」 with the number in the number face; other comments as they are (Mac ModernCommentText).</summary>
    private static TextBlock CommentText(string text, Brush brush, out bool mixed)
    {
        var parts = text.Split(' ', 2);
        mixed = parts.Length == 2 && parts[1].Any(char.IsDigit);
        var tb = mixed
            ? Theme.Block(Theme.R(parts[0], 11), Theme.Gap(3), Theme.R(parts[1], 11, Face.Mono))
            : Theme.Block(Theme.R(text, 11));
        tb.Foreground = brush;
        return tb;
    }

    /// <summary>「其余 N 个」 (132c, 132d): the folded subscriptions' marks and their summary, on 偏快's line.</summary>
    private static Grid Remainder(OverviewRemainder rest)
    {
        var g = new Grid { Width = Theme.RightColumnWidth, Height = 18 };
        var label = Theme.Block(
            Theme.R("其余 ", 13, Face.Text, Wt.Medium),
            Theme.R(rest.Providers.Count.ToString(System.Globalization.CultureInfo.InvariantCulture), 13, Face.Mono, Wt.Medium),
            Theme.R(" 个", 13, Face.Text, Wt.Medium));
        label.Foreground = Theme.Secondary;
        g.Children.Add(Theme.AtBaseline(label, Theme.BaselineIn(18, (13, Face.Text), (13, Face.Mono))));

        var marks = new StackPanel
        {
            Orientation = Orientation.Horizontal, Spacing = 8,
            HorizontalAlignment = HorizontalAlignment.Right, VerticalAlignment = VerticalAlignment.Center,
            Margin = new Thickness(0, 0, Theme.RightColumnWidth - Gauges.OverviewBarWidth, 0),
        };
        foreach (var p in rest.Providers)
        {
            var holder = new Grid { Width = 14, Height = 14, Background = Theme.Transparent };
            holder.Children.Add(QuotaIcons.Glyph(p, 14, Theme.Tertiary));
            ToolTipService.SetToolTip(holder, p.DisplayName());
            marks.Children.Add(holder);
        }
        g.Children.Add(marks);

        var summary = Theme.T(rest.Summary ?? "", 11, Face.Text, Wt.Regular, Toned(rest.Tone, Theme.Secondary));
        g.Children.Add(Theme.AtBaseline(summary, Theme.BaselineIn(18, (11, Face.Text)), HorizontalAlignment.Right));
        return g;
    }

    // -- one subscription (131b–131d, 132g, 132i–132r) ------------------------------------------

    private UIElement QuotaDetail(QuotaSubscriptionDisplay d, QuotaPanelDisplay quota)
    {
        var g = new Grid { MinHeight = Theme.CardBodyHeight };
        var ring = new Grid
        {
            Width = Gauges.RingSize, Height = Gauges.RingSize,
            HorizontalAlignment = HorizontalAlignment.Left, VerticalAlignment = VerticalAlignment.Top,
            Margin = new Thickness(-7, 0, 0, 0),
        };
        ring.Children.Add(Gauges.Ring(Gauge.Of(d.Outer), Gauge.Of(d.Inner), d.Outer == null && d.Inner == null));
        ring.Children.Add(RingCenter(d));
        g.Children.Add(ring);

        // The column is centred in the ring's height and grows past it (SwiftUI minHeight + .leading).
        var column = new StackPanel
        {
            Width = Theme.RightColumnWidth,
            HorizontalAlignment = HorizontalAlignment.Left, VerticalAlignment = VerticalAlignment.Center,
            Margin = new Thickness(Theme.LeadingColumnWidth, 0, 0, 0),
        };
        column.Children.Add(DetailHeader(d));
        if (d.Rows.Count == 0) column.Children.Add(DetailMessage(d, quota));
        else AddAllowances(column, d);
        g.Children.Add(column);
        return g;
    }

    private static UIElement RingCenter(QuotaSubscriptionDisplay d)
    {
        var g = new Grid { Width = Gauges.RingSize, Height = Gauges.RingSize };
        if (d.ShowsSkeleton)
        {
            var spinner = Gauges.RingSpinner();
            spinner.HorizontalAlignment = HorizontalAlignment.Center;
            spinner.VerticalAlignment = VerticalAlignment.Center;
            g.Children.Add(spinner);
        }
        else if (d.RingCenter is { Remaining: double remaining } center)
        {
            // 60 DIP (number 44, gap 2, caption 14) centred, 1 DIP low: the caption hangs under the number.
            double top = (Gauges.RingSize - 60) / 2 + 1;
            var number = Theme.Block(
                Theme.R(QuotaFormat.Percent(remaining), 44, Face.Mono, Wt.Light, Toned(center.Tone, Theme.Text), -2.64),
                Theme.Gap(2),
                Theme.R("%", 22, Face.Mono, Wt.Light, Theme.Secondary));
            g.Children.Add(Theme.AtBaseline(number, top + BigBaseline, HorizontalAlignment.Center));
            var caption = Theme.Block(Theme.R(center.Label, 11, Face.Mono), Theme.Gap(4), Theme.R("剩余", 11, Face.Text, Wt.Regular, null, 1.1));
            caption.Foreground = Theme.Secondary;
            g.Children.Add(Theme.AtBaseline(caption, top + 46 + Theme.BaselineIn(14, (11, Face.Mono), (11, Face.Text)), HorizontalAlignment.Center));
        }
        else
        {
            g.Children.Add(new Border
            {
                Width = 22, Height = 2, CornerRadius = new CornerRadius(1), Background = Theme.Solid(Theme.TextC(0.3)),
                HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center,
            });
        }
        return g;
    }

    /// <summary>Mark, name, plan; 重置券 at the right. 22 DIP, 1.5 DIP low, 10 DIP above the rows.</summary>
    private static Grid DetailHeader(QuotaSubscriptionDisplay d)
    {
        var g = new Grid { Height = 32 };
        bool credits = d.ResetCreditsCount is > 0;
        double baseline = 1.5 + Theme.BaselineFor(22, credits ? 13 : Theme.Ascent(13, Face.Text), credits ? 5 : Theme.Descent(13, Face.Text));
        g.Children.Add(Theme.At(QuotaIcons.Glyph(d.Provider, 15, Theme.Strong), 0, baseline - 4.5 - 7.5));
        var title = Theme.Block(Theme.R(d.Provider.DisplayName(), 13, Face.Text, Wt.SemiBold, Theme.Text, -0.13));
        if (d.PlanLabel is { } plan)
        {
            title.Inlines.Add(Theme.Gap(7));
            title.Inlines.Add(Theme.R(plan, 11, Face.Text, Wt.Regular, Theme.Secondary));
        }
        g.Children.Add(Theme.At(Theme.AtBaseline(title, baseline), 22, 0));
        if (credits)
        {
            var text = Theme.Block(Theme.R("重置券", 11), Theme.Gap(4), Theme.R("×" + d.ResetCreditsCount, 11, Face.Mono));
            text.Foreground = Theme.Strong;
            var inside = new Grid { Height = 18 };
            inside.Children.Add(Theme.AtBaseline(text, Theme.BaselineIn(18, (11, Face.Text), (11, Face.Mono))));
            var capsule = new Border
            {
                Height = 18, CornerRadius = new CornerRadius(9), Padding = new Thickness(8, 0, 8, 0),
                Background = Theme.Solid(Theme.TextC(0.08)), Child = inside,
                HorizontalAlignment = HorizontalAlignment.Right, VerticalAlignment = VerticalAlignment.Top,
                Margin = new Thickness(0, baseline - 13, 0, 0),
            };
            g.Children.Add(capsule);
        }
        return g;
    }

    private void AddAllowances(StackPanel column, QuotaSubscriptionDisplay d)
    {
        var shown = quotaExpanded ? d.Rows : d.CompactRows;
        double widest = shown.Count == 0 ? 0 : shown.Max(m => Theme.ApproximateWidth(m.Label, 11));
        double labelWidth = Math.Max(28, Math.Ceiling(widest + 7));
        foreach (var m in shown) column.Children.Add(AllowanceRow(m, d.Provider, labelWidth));
        if (d.FoldedCount > 0) column.Children.Add(FoldButton(d));
        if (d.SourceNote is { } note) column.Children.Add(SourceNote(note, d.RecordTimeText(TimeZoneInfo.Local, System.Globalization.CultureInfo.GetCultureInfo("zh-CN"))));
    }

    /// <summary>
    /// One allowance: label, remaining, the forecast, the reset. Hovering (or
    /// the keyboard on it) lights the row and hangs its tooltip under it.
    /// </summary>
    private FrameworkElement AllowanceRow(QuotaPanelMeter m, Provider provider, double labelWidth)
    {
        const double width = Theme.RightColumnWidth;
        var g = new Grid { Height = 32, Width = width, Background = Theme.Transparent };
        g.Children.Add(Theme.RuleLine());
        double baseline = 1 + Theme.BaselineIn(31, (13, Face.Mono), (11, Face.Mono), (11, Face.Text));
        var label = Theme.T(m.Label, 11, Face.Mono, Wt.Regular, Theme.Secondary);
        label.MaxWidth = labelWidth;
        label.TextTrimming = TextTrimming.Clip;
        g.Children.Add(Theme.AtBaseline(label, baseline));

        string spoken;
        if (m.Remaining is double remaining)
        {
            var number = Theme.Block(
                Theme.R(QuotaFormat.Percent(remaining), 13, Face.Mono, Wt.Medium, Theme.Text, -0.26),
                Theme.R("%", 13, Face.Mono, Wt.Regular, Theme.Tertiary));
            g.Children.Add(Theme.At(Theme.AtBaseline(number, baseline), labelWidth, 0));

            string? reset = QuotaFormat.Reset(m, TimeZoneInfo.Local);
            double resetWidth = 0;
            if (reset != null)
            {
                var resetText = Theme.Mixed(reset, 11, Theme.Secondary);
                var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 4, HorizontalAlignment = HorizontalAlignment.Right, VerticalAlignment = VerticalAlignment.Top };
                var icon = QuotaIcons.ArrowClockwise(Theme.Secondary);
                icon.VerticalAlignment = VerticalAlignment.Top;
                icon.Margin = new Thickness(0, baseline - 10.35, 0, 0);
                row.Children.Add(icon);
                row.Children.Add(Theme.AtBaseline(resetText, baseline));
                g.Children.Add(row);
                resetText.Measure(new Size(double.PositiveInfinity, double.PositiveInfinity));
                resetWidth = resetText.DesiredSize.Width + 4 + icon.Width;
            }

            string forecast = m.Prediction?.Text ?? "";
            var prediction = Theme.T(forecast, 11, Face.Text, Wt.Regular, Toned(m.Tone, Theme.Secondary));
            prediction.TextTrimming = TextTrimming.CharacterEllipsis;
            prediction.MaxWidth = Math.Max(0, width - labelWidth - 52 - 8 - resetWidth);
            g.Children.Add(Theme.At(Theme.AtBaseline(prediction, baseline), labelWidth + 52, 0));
            spoken = $"{m.Label} 剩余 {QuotaFormat.Percent(remaining)}%" + (forecast.Length > 0 ? "，" + forecast : "")
                + (reset != null ? "，" + (reset.Contains(':') ? reset + " 重置" : reset + "重置") : "");
        }
        else
        {
            var message = Theme.T(m.Message ?? "", 11, Face.Text, Wt.Regular, Theme.Secondary);
            g.Children.Add(Theme.At(Theme.AtBaseline(message, baseline), labelWidth, 0));
            spoken = $"{m.Label} {m.Message}";
        }

        // Not a button (a click does nothing): a focus stop that reads the row and shows its tooltip.
        var holder = new InfoRow { Content = g, IsTabStop = true, UseSystemFocusVisuals = true };
        AutomationProperties.SetName(holder, spoken);
        var tip = QuotaFormat.Tooltip(m);
        void Light(bool on)
        {
            if (on)
            {
                hoveredDetailRow = m.Id;
                ShowBand(holder, 32, 6, 0.05);
                Note("hover allowance-" + m.Id, "");
                if (tip == null) return;
                ShowTooltip(holder, tip);
                if (tooltipBlock != null) Note("tooltip allowance-" + m.Id, "");
            }
            else if (hoveredDetailRow == m.Id)
            {
                hoveredDetailRow = null;
                HideBand();
                HideTooltip();
            }
        }
        holder.PointerEntered += (_, _) => Light(true);
        holder.PointerExited += (_, _) => { if (holder.FocusState != FocusState.Keyboard) Light(false); };
        holder.PointerCanceled += (_, _) => Light(false);
        holder.GotFocus += (_, _) => { if (holder.FocusState == FocusState.Keyboard) Light(true); };
        holder.LostFocus += (_, _) => Light(false);
        if (hoveredDetailRow == m.Id)
        {
            if (tip != null) tooltip = (holder, tip);
            // Placed once the row has its size (a CI scene opens with it hovered).
            holder.SizeChanged += (_, _) =>
            {
                if (hoveredDetailRow != m.Id) return;
                ShowBand(holder, 32, 6, 0.05);
                if (tooltip is { } t && t.Row == holder) PlaceTooltip();
            };
        }
        return holder;
    }

    /// <summary>「另有 N 项」 / 「收起」 under the third row.</summary>
    private Button FoldButton(QuotaSubscriptionDisplay d)
    {
        var g = new Grid { Height = 20, Width = Theme.RightColumnWidth, Background = Theme.Transparent };
        g.Children.Add(Theme.RuleLine());
        var text = quotaExpanded
            ? Theme.Block(Theme.R("收起", 11))
            : Theme.Block(Theme.R("另有 ", 11), Theme.R(d.FoldedCount.ToString(System.Globalization.CultureInfo.InvariantCulture), 11, Face.Mono), Theme.R(" 项", 11));
        text.Foreground = Theme.Secondary;
        double baseline = quotaExpanded
            ? 1 + 2 + Theme.BaselineFor(17, Theme.Ascent(11, Face.Text), Theme.Descent(11, Face.Text))
            : 1 + 2 + Theme.BaselineFor(17, Theme.Ascent(11, Face.Mono), Theme.Descent(11, Face.Mono));
        var line = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6, VerticalAlignment = VerticalAlignment.Top };
        line.Children.Add(Theme.AtBaseline(text, baseline));
        var chevronBrush = Theme.Solid(Theme.TextC(0.62));
        var chevron = Icons.Chevron(quotaExpanded, chevronBrush, 6.2);
        chevron.VerticalAlignment = VerticalAlignment.Top;
        chevron.Margin = new Thickness(0, 11.5 - chevron.Height / 2, 0, 0);
        line.Children.Add(chevron);
        g.Children.Add(line);
        string name = quotaExpanded ? "收起" : $"另有 {d.FoldedCount} 项";
        return Bare(g, "fold", name, () =>
        {
            quotaExpanded = !quotaExpanded;
            hoveredDetailRow = null;
            ShellLog.Write($"quota rows {(quotaExpanded ? "unfolded" : "folded")}");
            Render();
        }, on =>
        {
            text.Foreground = on ? Theme.Text : Theme.Secondary;
            foreach (var p in chevron.Children.OfType<Path>()) p.Stroke = on ? Theme.Text : chevronBrush;
        });
    }

    /// <summary>Where kept numbers came from and why (132q); Claude Desktop's record also says when it was written.</summary>
    private static Grid SourceNote(string note, string? recordTime)
    {
        var g = new Grid { MinHeight = 22, Width = Theme.RightColumnWidth, Padding = new Thickness(0, 1 + 3.5, 0, 3.5) };
        g.Children.Add(new Rectangle { Height = 1, Fill = Theme.Rule, VerticalAlignment = VerticalAlignment.Top, Margin = new Thickness(0, -4.5, 0, 0) });
        var tb = Theme.T(note, 11, Face.Text, Wt.Regular, Theme.Secondary);
        if (recordTime != null)
        {
            tb.Inlines.Add(new Microsoft.UI.Xaml.Documents.LineBreak());
            tb.Inlines.Add(Theme.R(recordTime, 11));
        }
        tb.TextWrapping = TextWrapping.Wrap;
        tb.MaxLines = recordTime == null ? 2 : 3;
        tb.TextTrimming = TextTrimming.CharacterEllipsis;
        tb.LineHeight = Theme.LineHeight(11, Face.Text);
        tb.LineStackingStrategy = LineStackingStrategy.BlockLineHeight;
        tb.VerticalAlignment = VerticalAlignment.Center;
        g.Children.Add(tb);
        return g;
    }

    /// <summary>Nothing to draw (132m–132p, the special states): the state's title, its reason, 「重试」 or the way to Cursor's sign-in.</summary>
    private UIElement DetailMessage(QuotaSubscriptionDisplay d, QuotaPanelDisplay quota)
    {
        const double width = Theme.RightColumnWidth;
        var g = new Grid();
        g.Children.Add(Theme.RuleLine());
        var stack = new StackPanel { Padding = new Thickness(0, 9, 0, 0) };
        string title = d.DetailTitle ?? "";

        var titleLine = new Grid { MinHeight = 18 };
        if (d.Action == QuotaAction.CursorSettings)
        {
            // Cursor's own login cannot be read: the line leads to the sign-in in Settings.
            var restBrush = Theme.Solid(Theme.TextC(0.92));
            var text = Theme.T(title, 13, Face.Text, Wt.Medium, restBrush);
            var line = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6, MinHeight = 18 };
            var para = Theme.Paragraph(text, 13, Face.Text, width - 6 - 5, 3);
            para.VerticalAlignment = VerticalAlignment.Center;
            line.Children.Add(para);
            var chevron = QuotaIcons.ChevronRight(Theme.Solid(Theme.TextC(0.92 * 0.7)));
            line.Children.Add(chevron);
            titleLine.Children.Add(Bare(line, "cursor-signin", title, () => SettingsRequested?.Invoke(), on =>
            {
                text.Foreground = on ? Theme.Text : restBrush;
            }));
        }
        else
        {
            var line = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6, VerticalAlignment = VerticalAlignment.Center };
            double room = width;
            if (d.ReadState == ReadState.ReadFailed)
            {
                var alert = QuotaIcons.AlertCircle(Theme.RedBrush);
                alert.VerticalAlignment = VerticalAlignment.Top;
                alert.Margin = new Thickness(0, Theme.Ascent(13, Face.Text) - 4 - alert.Height / 2, 0, 0);
                line.Children.Add(alert);
                room -= alert.Width + 6;
            }
            var text = Theme.T(title, 13, Face.Text, Wt.Medium, Toned(d.CommentTone, Theme.Text));
            line.Children.Add(Theme.Paragraph(text, 13, Face.Text, room, 3));
            titleLine.Children.Add(line);
        }
        stack.Children.Add(titleLine);

        if (d.DetailReason is { } reason)
        {
            var r = Theme.Paragraph(Theme.T(reason, 11, Face.Text, Wt.Regular, Theme.Secondary), 11, Face.Text, width, 2);
            r.Margin = new Thickness(r.Margin.Left, r.Margin.Top + 2, r.Margin.Right, r.Margin.Bottom);
            stack.Children.Add(r);
        }
        if (d.Action == QuotaAction.Retry)
        {
            var label = Theme.T("重试", 11, Face.Text, Wt.Medium, Theme.Text);
            var inside = new Grid { Height = 22 };
            inside.Children.Add(Theme.AtBaseline(label, Theme.BaselineIn(22, (11, Face.Text))));
            var capsule = new Border { Height = 22, CornerRadius = new CornerRadius(11), Padding = new Thickness(11, 0, 11, 0), Background = Theme.Chip, Child = inside };
            var retry = Bare(capsule, "retry", "重试", () => model.RefreshQuotas(quota));
            retry.Margin = new Thickness(0, 10, 0, 0);
            stack.Children.Add(retry);
        }
        g.Children.Add(stack);
        return g;
    }

    // -- the allowance tooltip (132r) -------------------------------------------------------------

    private void ShowTooltip(FrameworkElement row, MeterTooltip tip)
    {
        tooltip = (row, tip);
        PlaceTooltip();
    }

    private void HideTooltip()
    {
        tooltip = null;
        RemoveTooltipBlock();
    }

    private void RemoveTooltipBlock()
    {
        if (tooltipBlock != null) overlay.Children.Remove(tooltipBlock);
        tooltipBlock = null;
    }

    /// <summary>
    /// Hangs the tooltip 2 DIP under the row's highlight, right edges aligned;
    /// above the row when the panel's bottom is too close (Mac ModernTooltipLayout).
    /// </summary>
    private void PlaceTooltip()
    {
        RemoveTooltipBlock();
        if (tooltip is not { } shown || drag?.Active == true) return;
        var (row, tip) = shown;
        if (row.XamlRoot == null || row.ActualWidth <= 0) return;
        Rect r;
        try
        {
            r = row.TransformToVisual(root).TransformBounds(new Rect(0, 0, row.ActualWidth, row.ActualHeight));
        }
        catch (ArgumentException)
        {
            return;
        }
        var block = TooltipBlock(tip, out double w, out double h);
        double inset = 8, right = r.Right + 8;
        double x = Math.Min(Math.Max(inset, right - w), Math.Max(inset, root.ActualWidth - w - inset));
        double below = r.Bottom + 2, above = r.Top - 2 - h;
        double y = below + h + inset <= root.ActualHeight ? below : Math.Max(inset, above);
        Canvas.SetLeft(block, Math.Round(x));
        Canvas.SetTop(block, Math.Round(y));
        overlay.Children.Add(block);
        tooltipBlock = block;
    }

    /// <summary>The window, then 已使用 / 已过去 / 离重置 and the allowance's own split, on the raised block.</summary>
    private static FrameworkElement TooltipBlock(MeterTooltip tip, out double width, out double height)
    {
        width = 156 + 20;
        height = 7 + 16 + 2 + 16 * tip.Lines.Count + 8;
        var g = new Grid { Width = width, Height = height };
        var shadowHost = new Grid();
        var bg = new Rectangle { RadiusX = 8, RadiusY = 8, Fill = Theme.VerticalGradient(Theme.BlockTop, Theme.BlockBottom) };
        g.Children.Add(shadowHost);
        g.Children.Add(bg);
        Gauges.DropShadow(shadowHost, bg, width, height, 22, 8, 0.5f);
        g.Children.Add(new Border
        {
            CornerRadius = new CornerRadius(8), BorderThickness = new Thickness(1),
            BorderBrush = Theme.AbsoluteVerticalGradient(Theme.Hex(0xFFFFFF, 0.08), Theme.Hex(0xFFFFFF, 0), 0, 8),
        });
        g.Children.Add(new Border { CornerRadius = new CornerRadius(8), BorderThickness = new Thickness(1), BorderBrush = Theme.Solid(Theme.Hex(0x000000, 0.45)) });

        g.Children.Add(Theme.At(Theme.AtBaseline(Theme.T(tip.Title, 11, Face.Text, Wt.Medium, Theme.Text),
            7 + Theme.BaselineIn(16, (11, Face.Text))), 10, 0));
        for (int i = 0; i < tip.Lines.Count; i++)
        {
            double top = 7 + 16 + 2 + 16 * i;
            double baseline = top + Theme.BaselineIn(16, (11, Face.Text), (11, Face.Mono));
            var valueText = Theme.Mixed(tip.Lines[i].Value, 11, Theme.Text);
            valueText.Measure(new Size(double.PositiveInfinity, double.PositiveInfinity));
            var label = Theme.T(tip.Lines[i].Label, 11, Face.Text, Wt.Regular, Theme.Secondary);
            label.MaxWidth = Math.Max(0, 156 - 14 - valueText.DesiredSize.Width);
            label.TextTrimming = TextTrimming.CharacterEllipsis;
            g.Children.Add(Theme.At(Theme.AtBaseline(label, baseline), 10, 0));
            var value = Theme.AtBaseline(valueText, baseline, HorizontalAlignment.Right);
            value.Margin = new Thickness(0, 0, 10, 0);
            g.Children.Add(value);
        }
        g.IsHitTestVisible = false;
        return g;
    }

    // -- round buttons (133) ----------------------------------------------------------------------

    private sealed class Drag
    {
        public required Provider Provider { get; init; }
        public required Point Start { get; init; }
        public required Point Grab { get; init; }
        public bool Active { get; set; }
        public bool Released { get; set; }
        public FrameworkElement? Ghost { get; set; }
    }

    private Drag? drag;
    private bool renderPending;
    private StackPanel? roundButtons;
    private readonly List<(Provider Provider, FrameworkElement Element)> roundButtonList = [];

    /// <summary>
    /// The round buttons: one per enabled subscription in the order Settings
    /// keeps. A click opens that subscription; dragging one and dropping it on
    /// another puts it before that one, past the last puts it last
    /// (Mac ModernSubscriptionButtons). The native tooltip says name, plan and state.
    /// </summary>
    private StackPanel RoundButtons(QuotaPanelDisplay quota)
    {
        var buttons = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6, VerticalAlignment = VerticalAlignment.Center };
        roundButtons = buttons;
        roundButtonList.Clear();
        foreach (var s in quota.Buttons)
        {
            bool selected = quota.Detail?.Provider == s.Provider;
            var provider = s.Provider;
            var holder = new Grid { Width = Gauges.ButtonSize, Height = Gauges.ButtonSize, Background = Theme.Transparent };
            holder.Children.Add(Gauges.RoundButtonFace(provider, s.ButtonRemaining, selected, s.ShowsWhiteDot, false));
            var b = Bare(holder, "sub-" + provider, s.ButtonHelp, () =>
            {
                if (drag?.Active == true) return;
                model.Select(provider);
            }, on =>
            {
                holder.Children.Clear();
                holder.Children.Add(Gauges.RoundButtonFace(provider, s.ButtonRemaining, selected, s.ShowsWhiteDot, on));
            }, s.ButtonHelp);
            if (selected) AutomationProperties.SetItemStatus(b, "已选中");
            b.AddHandler(UIElement.PointerPressedEvent, new PointerEventHandler((_, e) => DragPressed(provider, b, e)), true);
            b.AddHandler(UIElement.PointerMovedEvent, new PointerEventHandler((_, e) => DragMoved(s, e)), true);
            b.AddHandler(UIElement.PointerReleasedEvent, new PointerEventHandler((_, e) => DragReleased(e)), true);
            b.PointerCaptureLost += (_, _) =>
            {
                var lost = drag;
                DispatcherQueue.TryEnqueue(() => { if (drag == lost && lost is { Released: false }) EndDrag(); });
            };
            buttons.Children.Add(b);
            roundButtonList.Add((provider, b));
        }
        return buttons;
    }

    private void DragPressed(Provider provider, Button b, PointerRoutedEventArgs e)
    {
        var point = e.GetCurrentPoint(root);
        if (!point.Properties.IsLeftButtonPressed) return;
        var local = e.GetCurrentPoint(b).Position;
        drag = new Drag { Provider = provider, Start = point.Position, Grab = local };
    }

    private void DragMoved(QuotaSubscriptionDisplay s, PointerRoutedEventArgs e)
    {
        if (drag is not { } d || d.Provider != s.Provider) return;
        var p = e.GetCurrentPoint(root).Position;
        if (!d.Active)
        {
            if (Math.Abs(p.X - d.Start.X) < 4 && Math.Abs(p.Y - d.Start.Y) < 4) return;
            d.Active = true;
            var ghost = Gauges.RoundButtonFace(s.Provider, s.ButtonRemaining, false, s.ShowsWhiteDot, false);
            ghost.Opacity = 0.8;
            d.Ghost = ghost;
            HideTooltip();
            overlay.Children.Add(ghost);
            ShellLog.Write($"drag start {s.Provider}");
        }
        if (d.Ghost != null)
        {
            Canvas.SetLeft(d.Ghost, p.X - d.Grab.X);
            Canvas.SetTop(d.Ghost, p.Y - d.Grab.Y);
        }
    }

    private void DragReleased(PointerRoutedEventArgs e)
    {
        if (drag is not { } d) return;
        d.Released = true;
        if (d.Active && roundButtons != null)
        {
            var p = e.GetCurrentPoint(roundButtons).Position;
            if (TryDropTarget(p, out var before))
            {
                ShellLog.Write($"drop {d.Provider} before {(before?.ToString() ?? "end")}");
                model.MoveSubscription(d.Provider, before);
            }
            else
            {
                ShellLog.Write($"drop {d.Provider} outside");
            }
        }
        EndDrag();
    }

    /// <summary>
    /// Where a drop at <paramref name="p"/> (in the buttons' coordinates)
    /// lands: on a button → before it; on the row's free width past the last
    /// button → last; anywhere else → no drop.
    /// </summary>
    private bool TryDropTarget(Point p, out Provider? before)
    {
        before = null;
        if (p.Y < -10 || p.Y > Gauges.ButtonSize + 10 || roundButtonList.Count == 0 || roundButtons == null) return false;
        foreach (var (provider, element) in roundButtonList)
        {
            var at = element.TransformToVisual(roundButtons).TransformPoint(new Point(0, 0));
            if (p.X >= at.X - 3 && p.X <= at.X + element.ActualWidth + 3)
            {
                before = provider;
                return true;
            }
        }
        // The free width between the last button and the gear.
        var last = roundButtonList[^1].Element;
        double end = last.TransformToVisual(roundButtons).TransformPoint(new Point(last.ActualWidth, 0)).X;
        double rowX = roundButtons.TransformToVisual(root).TransformPoint(new Point(0, 0)).X;
        return p.X > end && p.X + rowX < Theme.PanelWidth - Theme.TextInset - 22;
    }

    private void EndDrag()
    {
        if (drag?.Ghost is { } ghost) overlay.Children.Remove(ghost);
        drag = null;
        if (renderPending)
        {
            renderPending = false;
            Render();
        }
    }
}

/// <summary>
/// A row that only reports (an allowance): a keyboard stop Narrator reads as
/// text, not a button, since a click does nothing.
/// </summary>
internal sealed partial class InfoRow : UserControl
{
    protected override Microsoft.UI.Xaml.Automation.Peers.AutomationPeer OnCreateAutomationPeer() => new Peer(this);

    private sealed partial class Peer(InfoRow owner) : Microsoft.UI.Xaml.Automation.Peers.FrameworkElementAutomationPeer(owner)
    {
        protected override Microsoft.UI.Xaml.Automation.Peers.AutomationControlType GetAutomationControlTypeCore() =>
            Microsoft.UI.Xaml.Automation.Peers.AutomationControlType.Text;

        protected override string GetClassNameCore() => "QuotaAllowanceRow";

        protected override bool IsKeyboardFocusableCore() => true;
    }
}

/// <summary>The card's small marks, after the Mac's SF Symbols at the same sizes.</summary>
internal static class QuotaIcons
{
    private static Canvas Box(double w, double h, params UIElement[] children)
    {
        var c = new Canvas { Width = w, Height = h, VerticalAlignment = VerticalAlignment.Center, IsHitTestVisible = false };
        foreach (var ch in children) c.Children.Add(ch);
        return c;
    }

    /// <summary>SF "chevron.right" at 8 pt semibold.</summary>
    public static Canvas ChevronRight(Brush brush)
    {
        const double h = 7, w = 3.85;
        return Box(w, h, Geo.Stroke(Geo.Geometry(Geo.Poly(false, 0.4, 0.6, w - 0.4, h / 2, 0.4, h - 0.6)), brush, 1.25));
    }

    /// <summary>
    /// SF "arrow.clockwise" at 10 pt medium, in its 10.45 × 12 layout box: a
    /// circle open at the upper right, an open arrowhead at 12 o'clock
    /// pointing on (measured on the symbol drawn at 10×).
    /// </summary>
    public static Canvas ArrowClockwise(Brush brush)
    {
        const double cx = 5.2, cy = 7.4, r = 3.7, w = 1.1;
        var arc = Geo.Stroke(Geo.Arc(cx, cy, r, 95, 360), brush, w);
        double tx = cx + 2.1, ty = cy - r;
        var head = Geo.Stroke(Geo.Geometry(Geo.Poly(false, tx - 2.3, ty - 2.3, tx, ty, tx - 2.3, ty + 2.3)), brush, w);
        return Box(10.45, 12, arc, head);
    }

    /// <summary>
    /// SF "arrow.2.circlepath" at 10.5 pt medium in a 14 × 14 frame: two
    /// arcs chasing round, each ending in a solid head (measured on the
    /// symbol drawn at 10×).
    /// </summary>
    public static Canvas Refresh(Brush brush)
    {
        const double cx = 7, cy = 7.1, r = 4.55, w = 1.1;
        var c = Box(14, 14,
            Geo.Stroke(Geo.Arc(cx, cy, r, 311, 440), brush, w),
            Geo.Stroke(Geo.Arc(cx, cy, r, 131, 260), brush, w),
            Head(cx, cy, r, 80, brush),
            Head(cx, cy, r, 260, brush));
        return c;
    }

    /// <summary>A solid arrowhead whose base sits across the circle at <paramref name="deg"/>, pointing clockwise.</summary>
    private static Path Head(double cx, double cy, double r, double deg, Brush brush)
    {
        double a = deg * Math.PI / 180;
        var b = Geo.Polar(cx, cy, r, deg);
        double tx = Math.Cos(a), ty = Math.Sin(a), nx = Math.Sin(a), ny = -Math.Cos(a);
        const double half = 1.6, length = 2.1;
        return new Path
        {
            Data = Geo.Geometry(Geo.Poly(true, b.X + nx * half, b.Y + ny * half, b.X + tx * length, b.Y + ty * length, b.X - nx * half, b.Y - ny * half)),
            Fill = brush,
        };
    }

    /// <summary>Recolours a drawn icon (hover).</summary>
    public static void Paint(Canvas icon, Brush brush)
    {
        foreach (var p in icon.Children.OfType<Path>())
        {
            if (p.Fill != null) p.Fill = brush;
            if (p.Stroke != null) p.Stroke = brush;
        }
    }

    /// <summary>
    /// A provider mark as the new panel draws it (Mac <c>ProviderGlyph</c>):
    /// <paramref name="size"/> × the mark's fill ratio, centred in a
    /// size × size box. (The classic card's icon is a further 18/22 smaller.)
    /// </summary>
    public static FrameworkElement Glyph(Provider provider, double size, Brush brush)
    {
        double drawn = size * ProviderMarks.DrawnSize(provider, 22) / 18;
        var holder = new Grid { Width = size, Height = size, IsHitTestVisible = false };
        var inner = new Canvas { Width = 24, Height = 24 };
        inner.Children.Add(new Path { Data = Geo.FromMarkup(ProviderMarks.Markup(provider)), Fill = brush });
        holder.Children.Add(new Viewbox
        {
            Width = drawn, Height = drawn, Child = inner,
            HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center,
        });
        return holder;
    }

    /// <summary>SF "exclamationmark.circle" at 11 pt medium, in its 13 × 13 frame.</summary>
    public static Canvas AlertCircle(Brush brush)
    {
        var ring = new Ellipse { Width = 12.2, Height = 12.2, Stroke = brush, StrokeThickness = 1.2 };
        Canvas.SetLeft(ring, 0.4);
        Canvas.SetTop(ring, 0.4);
        var dot = new Ellipse { Width = 1.6, Height = 1.6, Fill = brush };
        Canvas.SetLeft(dot, 5.7);
        Canvas.SetTop(dot, 8.7);
        return Box(13, 13, ring, Geo.Stroke(Geo.Geometry(Geo.Poly(false, 6.5, 3.6, 6.5, 7.3)), brush, 1.3), dot);
    }
}
