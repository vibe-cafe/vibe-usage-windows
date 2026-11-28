using System;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;

namespace VibeUsage.App.Panel;

/// <summary>Text placement the 订阅配额 card needs beyond <see cref="Theme"/>'s basics.</summary>
internal static partial class Theme
{
    /// <summary>A text's line box (ascent + descent) as CoreText measures it.</summary>
    public static double LineHeight(double size, Face face) => Ascent(size, face) + Descent(size, face);

    /// <summary>
    /// Where a first-baseline row's baseline sits in a frame
    /// <paramref name="height"/> tall when its parts reach
    /// <paramref name="above"/> over and <paramref name="below"/> under the
    /// baseline (SwiftUI centres that box in the frame).
    /// </summary>
    public static double BaselineFor(double height, double above, double below) => (height - (above + below)) / 2 + above;

    /// <summary>Width of a short label as the Mac lays out (ModernQuotaFormat.approximateWidth): ASCII 0.6 em, the rest 1 em.</summary>
    public static double ApproximateWidth(string text, double size)
    {
        double w = 0;
        foreach (char ch in text) w += ch < 128 ? 0.6 * size : size;
        return w;
    }

    /// <summary>
    /// A text that may need more than one line in <paramref name="width"/>
    /// (SwiftUI <c>.fixedSize(horizontal: false, vertical: true)</c> with
    /// <c>.lineSpacing</c>). One line keeps the Mac's exact baseline inside
    /// its line box; longer text wraps with the same line pitch.
    /// </summary>
    public static FrameworkElement Paragraph(TextBlock tb, double size, Face face, double width, double lineSpacing)
    {
        tb.Measure(new Windows.Foundation.Size(double.PositiveInfinity, double.PositiveInfinity));
        double line = LineHeight(size, face);
        double natural = tb.DesiredSize.Width;
        if (natural <= width + 0.5)
        {
            var box = new Grid { Height = line, HorizontalAlignment = HorizontalAlignment.Left };
            var at = AtBaseline(tb, Ascent(size, face));
            box.Children.Add(at);
            return box;
        }
        if (natural <= width + OneLineOverhang)
        {
            // Microsoft YaHei sets a line a few DIP wider than PingFang: keep the
            // Mac's single line and let it reach into the card's padding. In a
            // canvas, so layout does not clip it to the column.
            var line1 = new Grid { Width = Math.Ceiling(natural) + 1, Height = line };
            line1.Children.Add(AtBaseline(tb, Ascent(size, face)));
            var host = new Canvas { Width = width, Height = line, HorizontalAlignment = HorizontalAlignment.Left };
            host.Children.Add(line1);
            return host;
        }
        tb.TextWrapping = TextWrapping.Wrap;
        tb.Width = width;
        tb.LineHeight = line + lineSpacing;
        tb.LineStackingStrategy = LineStackingStrategy.BlockLineHeight;
        tb.Margin = new Thickness(0, -lineSpacing / 2, 0, -lineSpacing / 2);
        tb.VerticalAlignment = VerticalAlignment.Top;
        tb.HorizontalAlignment = HorizontalAlignment.Left;
        return tb;
    }

    /// <summary>How far past its column a title may run to stay on one line (font differences, not longer text).</summary>
    public const double OneLineOverhang = 6;

    /// <summary>Positions an element at (left, top) in a Grid cell, sized by its content.</summary>
    public static T At<T>(T e, double left, double top) where T : FrameworkElement
    {
        e.HorizontalAlignment = HorizontalAlignment.Left;
        e.VerticalAlignment = VerticalAlignment.Top;
        e.Margin = new Thickness(left, top, 0, 0);
        return e;
    }

    /// <summary>The 8 % line between rows (Mac <c>ModernRule</c>), 1 DIP at the top of its cell.</summary>
    public static Microsoft.UI.Xaml.Shapes.Rectangle RuleLine() => new()
    {
        Height = 1,
        Fill = Rule,
        VerticalAlignment = VerticalAlignment.Top,
        HorizontalAlignment = HorizontalAlignment.Stretch,
        IsHitTestVisible = false,
    };

}
