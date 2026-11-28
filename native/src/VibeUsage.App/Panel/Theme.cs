using System;
using Microsoft.UI;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Documents;
using Microsoft.UI.Xaml.Media;
using Windows.UI;

namespace VibeUsage.App.Panel;

public enum Face { Text, Mono }

public enum Wt { Light, Regular, Medium, SemiBold }

/// <summary>
/// The macOS new panel's theme (vibe-usage-app <c>ModernTheme.swift</c> and
/// the constants next to each view), values copied as they are: points become
/// DIPs. Dark only, like the Mac panel. Text never scales with the system
/// text size here (fixed layout, #85 「已按设计意图定下」 7).
/// </summary>
internal static partial class Theme
{
    // -- metrics (ModernTheme) ------------------------------------------------------------
    public const double PanelWidth = 520;
    public const double CardInset = 12;
    public const double CardPadding = 16;
    public const double TextInset = 28;
    public const double CardSpacing = 8;
    public const double CardCornerRadius = 10;
    public const double LeadingColumnWidth = 160;
    public const double CardBodyHeight = 148;
    /// <summary>Width of a card body's right column: 520 − 2×12 − 2×16 − 160.</summary>
    public const double RightColumnWidth = PanelWidth - 2 * CardInset - 2 * CardPadding - LeadingColumnWidth;

    // -- colours ------------------------------------------------------------------------------
    public static Color Hex(uint rgb, double alpha = 1) =>
        ColorHelper.FromArgb((byte)Math.Round(Math.Clamp(alpha, 0, 1) * 255), (byte)(rgb >> 16), (byte)(rgb >> 8), (byte)rgb);

    public static Color TextC(double alpha = 1) => Hex(0xE7EBEE, alpha);

    /// <summary>Warm white of ticks, rings, bars and curves: rgb(237,233,225).</summary>
    public static Color InkC(double alpha = 1) => Hex(0xEDE9E1, alpha);

    public static SolidColorBrush Solid(Color c) => new(c);

    public static readonly Color Panel = Hex(0x131619);
    public static readonly Color CardTop = Hex(0x1E2227);
    public static readonly Color CardBottom = Hex(0x181B1F);
    public static readonly Color Well = Hex(0x0D1013);
    public static readonly Color LampOn = Hex(0xEEF0F2);
    public static readonly Color LampOff = Hex(0x2B3036);
    public static readonly Color Amber = Hex(0xE7B369);
    public static readonly Color Red = Hex(0xE8796C);
    public static readonly Color RankBlue = Hex(0x608ECA);
    public static readonly Color RankTeal = Hex(0x4EB397);
    public static readonly Color RankMagenta = Hex(0x9C5B88);
    public static readonly Color RankGray = TextC(0.26);
    public static readonly Color BlockTop = Hex(0x2A2E34);
    public static readonly Color BlockBottom = Hex(0x22262B);
    public static readonly Color MarkerFill = Hex(0x1B1E23);
    /// <summary>The panel window's faint light edge (Mac #383A3D), as COLORREF for DWMWA_BORDER_COLOR.</summary>
    public const int BorderColorRef = 0x003D3A38;

    public static readonly SolidColorBrush Text = Solid(TextC());
    public static readonly SolidColorBrush Strong = Solid(TextC(0.86));
    public static readonly SolidColorBrush Secondary = Solid(TextC(0.62));
    public static readonly SolidColorBrush Tertiary = Solid(TextC(0.5));
    public static readonly SolidColorBrush Separator = Solid(TextC(0.14));
    public static readonly SolidColorBrush Chip = Solid(TextC(0.1));
    public static readonly SolidColorBrush Placeholder = Solid(TextC(0.07));
    public static readonly SolidColorBrush Rule = Solid(TextC(0.08));
    public static readonly SolidColorBrush AmberBrush = Solid(Amber);
    public static readonly SolidColorBrush RedBrush = Solid(Red);
    public static readonly SolidColorBrush Transparent = Solid(Colors.Transparent);

    // -- type -----------------------------------------------------------------------------------
    // The bundled faces (SIL OFL, Assets/Fonts). Neither has Chinese: the
    // second family in each list is the same file under its legacy name (in
    // case the text stack matches name ID 1 rather than the typographic
    // family), then Microsoft YaHei UI for Chinese, as PingFang is on macOS.
    private static FontFamily Family(string file, string family, string legacy) =>
        new($"ms-appx:///Assets/Fonts/{file}#{family}, ms-appx:///Assets/Fonts/{file}#{legacy}, Microsoft YaHei UI");

    private static readonly FontFamily TextRegular = new("ms-appx:///Assets/Fonts/SchibstedGrotesk-Regular.ttf#Schibsted Grotesk, Microsoft YaHei UI");
    private static readonly FontFamily TextMedium = Family("SchibstedGrotesk-Medium.ttf", "Schibsted Grotesk", "Schibsted Grotesk Medium");
    private static readonly FontFamily TextSemiBold = Family("SchibstedGrotesk-SemiBold.ttf", "Schibsted Grotesk", "Schibsted Grotesk SemiBold");
    private static readonly FontFamily MonoLight = Family("JetBrainsMono-Light.ttf", "JetBrains Mono", "JetBrains Mono Light");
    private static readonly FontFamily MonoRegular = new("ms-appx:///Assets/Fonts/JetBrainsMono-Regular.ttf#JetBrains Mono, Microsoft YaHei UI");
    private static readonly FontFamily MonoMedium = Family("JetBrainsMono-Medium.ttf", "JetBrains Mono", "JetBrains Mono Medium");

    /// <summary>ModernTheme.text / .mono: semibold and bold map to SemiBold, thin weights to Light (mono only).</summary>
    public static (FontFamily Family, Windows.UI.Text.FontWeight Weight) Font(Face face, Wt weight) => face switch
    {
        Face.Mono => weight switch
        {
            Wt.Light => (MonoLight, FontWeights.Light),
            Wt.Medium or Wt.SemiBold => (MonoMedium, FontWeights.Medium),
            _ => (MonoRegular, FontWeights.Normal),
        },
        _ => weight switch
        {
            Wt.SemiBold => (TextSemiBold, FontWeights.SemiBold),
            Wt.Medium => (TextMedium, FontWeights.Medium),
            _ => (TextRegular, FontWeights.Normal),
        },
    };

    /// <summary>A run; tracking is SwiftUI's .tracking in points.</summary>
    public static Run R(string text, double size, Face face = Face.Text, Wt weight = Wt.Regular,
        Brush? brush = null, double tracking = 0)
    {
        var (family, w) = Font(face, weight);
        var run = new Run { Text = text, FontFamily = family, FontWeight = w, FontSize = size };
        if (brush != null) run.Foreground = brush;
        if (tracking != 0) run.CharacterSpacing = (int)Math.Round(tracking / size * 1000);
        return run;
    }

    /// <summary>A gap of exactly <paramref name="width"/> DIPs between runs (a mono space at the right size).</summary>
    public static Run Gap(double width)
    {
        var (family, w) = Font(Face.Mono, Wt.Regular);
        return new Run { Text = " ", FontFamily = family, FontWeight = w, FontSize = Math.Max(0.5, width / 0.6) };
    }

    public static TextBlock Block(params Inline[] runs)
    {
        var tb = new TextBlock
        {
            TextWrapping = TextWrapping.NoWrap,
            IsTextScaleFactorEnabled = false,
            VerticalAlignment = VerticalAlignment.Center,
            Foreground = Text,
        };
        foreach (var r in runs) tb.Inlines.Add(r);
        return tb;
    }

    public static TextBlock T(string text, double size, Face face = Face.Text, Wt weight = Wt.Regular,
        Brush? brush = null, double tracking = 0)
    {
        var tb = Block(R(text, size, face, weight, null, tracking));
        if (brush != null) tb.Foreground = brush;
        return tb;
    }

    // -- baselines --------------------------------------------------------------------------------
    // SwiftUI centres a text's line box in its frame; CoreText takes the line
    // box from the hhea metrics (Schibsted Grotesk 2000/528 of 2048, JetBrains
    // Mono 1020/300 of 1000). Text here is placed by its baseline at the point
    // the Mac computes, which keeps rows level whatever metrics the Windows
    // text stack uses for the line box.

    public static double Ascent(double size, Face face) => size * (face == Face.Mono ? 1.02 : 2000.0 / 2048);

    public static double Descent(double size, Face face) => size * (face == Face.Mono ? 0.30 : 528.0 / 2048);

    /// <summary>Where the shared baseline of these runs sits in a frame <paramref name="height"/> tall.</summary>
    public static double BaselineIn(double height, params (double Size, Face Face)[] runs)
    {
        double asc = 0, desc = 0;
        foreach (var (size, face) in runs)
        {
            asc = Math.Max(asc, Ascent(size, face));
            desc = Math.Max(desc, Descent(size, face));
        }
        return (height - (asc + desc)) / 2 + asc;
    }

    /// <summary>
    /// A box from y = 0 to the baseline with the text's baseline on its bottom
    /// edge (TextLineBounds.TrimToBaseline). Put it at the top of the area the
    /// baseline is measured in. The negative top margin keeps tall glyphs from
    /// being clipped when they rise above the box.
    /// </summary>
    public static Grid AtBaseline(TextBlock tb, double baselineY, HorizontalAlignment align = HorizontalAlignment.Left)
    {
        tb.TextLineBounds = TextLineBounds.TrimToBaseline;
        tb.VerticalAlignment = VerticalAlignment.Bottom;
        tb.HorizontalAlignment = align;
        tb.Margin = new Thickness(0, -80, 0, 0);
        var g = new Grid { Height = Math.Max(0, baselineY), VerticalAlignment = VerticalAlignment.Top };
        g.Children.Add(tb);
        return g;
    }

    /// <summary>Words set closer than the font's space (RecentUsageWords): 「3h 12m」, 「昨天 16:00」.</summary>
    public static TextBlock Words(string text, double size, double spacing, Face face = Face.Mono, Wt weight = Wt.Regular, Brush? brush = null)
    {
        var tb = Block();
        if (brush != null) tb.Foreground = brush;
        var words = text.Split(' ', StringSplitOptions.RemoveEmptyEntries);
        for (int i = 0; i < words.Length; i++)
        {
            if (i > 0) tb.Inlines.Add(Gap(spacing));
            tb.Inlines.Add(R(words[i], size, face, weight));
        }
        return tb;
    }

    /// <summary>SwiftUI's .frame(height:): the text centred in a fixed-height box.</summary>
    public static Grid Line(UIElement content, double height, HorizontalAlignment align = HorizontalAlignment.Left)
    {
        var g = new Grid { Height = height };
        if (content is FrameworkElement fe)
        {
            fe.VerticalAlignment = VerticalAlignment.Center;
            fe.HorizontalAlignment = align;
        }
        g.Children.Add(content);
        return g;
    }

    /// <summary>
    /// Digits and their marks in the number face, the rest in the text face
    /// (ModernMixedNumberText): 「3 天 17 小时」, 「15:18」.
    /// </summary>
    public static TextBlock Mixed(string text, double size, Brush? brush = null)
    {
        var tb = Block();
        if (brush != null) tb.Foreground = brush;
        var current = new System.Text.StringBuilder();
        bool? number = null;
        void Flush()
        {
            if (current.Length == 0) return;
            tb.Inlines.Add(R(current.ToString(), size, number == true ? Face.Mono : Face.Text));
            current.Clear();
        }
        foreach (char c in text)
        {
            bool isNumber = char.IsDigit(c) || "%.<:/×".IndexOf(c) >= 0;
            if (number != null && number != isNumber) Flush();
            number = isNumber;
            current.Append(c);
        }
        Flush();
        return tb;
    }

    // -- surfaces -------------------------------------------------------------------------------

    /// <summary>
    /// The card surface: a top-to-bottom gradient, a 7% white line along the
    /// top edge that fades down the corners, and a tight drop shadow (drawn as
    /// a 1 DIP darker copy under the card).
    /// </summary>
    public static Grid Card(UIElement content, Thickness? padding = null)
    {
        var host = new Grid();
        host.Children.Add(new Border
        {
            CornerRadius = new CornerRadius(CardCornerRadius),
            Background = Solid(Hex(0x000000, 0.45)),
            Margin = new Thickness(0, 1, 0, -1),
        });
        var card = new Border
        {
            CornerRadius = new CornerRadius(CardCornerRadius),
            Background = VerticalGradient(CardTop, CardBottom),
            BorderThickness = new Thickness(1),
            BorderBrush = AbsoluteVerticalGradient(Hex(0xFFFFFF, 0.07), Hex(0xFFFFFF, 0), 0, CardCornerRadius),
            Padding = padding ?? new Thickness(CardPadding - 1),
            Child = content,
        };
        host.Children.Add(card);
        return host;
    }

    public static LinearGradientBrush VerticalGradient(Color top, Color bottom)
    {
        var b = new LinearGradientBrush { StartPoint = new Windows.Foundation.Point(0, 0), EndPoint = new Windows.Foundation.Point(0, 1) };
        b.GradientStops.Add(new GradientStop { Color = top, Offset = 0 });
        b.GradientStops.Add(new GradientStop { Color = bottom, Offset = 1 });
        return b;
    }

    public static LinearGradientBrush AbsoluteVerticalGradient(Color from, Color to, double y0, double y1)
    {
        var b = new LinearGradientBrush
        {
            MappingMode = BrushMappingMode.Absolute,
            StartPoint = new Windows.Foundation.Point(0, y0),
            EndPoint = new Windows.Foundation.Point(0, y1),
        };
        b.GradientStops.Add(new GradientStop { Color = from, Offset = 0 });
        b.GradientStops.Add(new GradientStop { Color = to, Offset = 1 });
        return b;
    }

    public static LinearGradientBrush AbsoluteHorizontalGradient(Color from, Color to, double x0, double x1)
    {
        var b = new LinearGradientBrush
        {
            MappingMode = BrushMappingMode.Absolute,
            StartPoint = new Windows.Foundation.Point(x0, 0),
            EndPoint = new Windows.Foundation.Point(Math.Max(x1, x0 + 1), 0),
        };
        b.GradientStops.Add(new GradientStop { Color = from, Offset = 0 });
        b.GradientStops.Add(new GradientStop { Color = to, Offset = 1 });
        return b;
    }

    /// <summary>The dark raised surface of hover blocks and tooltips (RecentUsageBlockBackground).</summary>
    public static Border RaisedBlock(UIElement child, double cornerRadius, Thickness padding) => new()
    {
        CornerRadius = new CornerRadius(cornerRadius),
        Background = VerticalGradient(BlockTop, BlockBottom),
        BorderThickness = new Thickness(1),
        BorderBrush = Solid(Hex(0x000000, 0.45)),
        Padding = padding,
        Child = child,
    };

    public static Color Lerp(Color a, Color b, double t) => ColorHelper.FromArgb(
        (byte)Math.Round(a.A + (b.A - a.A) * t), (byte)Math.Round(a.R + (b.R - a.R) * t),
        (byte)Math.Round(a.G + (b.G - a.G) * t), (byte)Math.Round(a.B + (b.B - a.B) * t));
}
