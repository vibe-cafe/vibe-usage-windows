using System;
using System.Numerics;
using Microsoft.UI.Composition;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Hosting;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Shapes;
using VibeUsage.Display;
using Windows.Foundation;
using Windows.UI;

namespace VibeUsage.App.Panel;

/// <summary>
/// One meter as a gauge draws it (Mac <c>ModernGaugeValue</c>): how much is
/// left and where time says it should be, both 0–1, and its tone. Built from
/// the display library's meter, never computed here.
/// </summary>
internal readonly record struct Gauge(double Remaining, double? Expected, PaceTone Tone)
{
    public static Gauge? Of(QuotaPanelMeter? m) => m?.Remaining is double r
        ? new Gauge(Math.Clamp(r / 100, 0, 1), m.ExpectedRemaining is double e ? Math.Clamp(e / 100, 0, 1) : null, m.Tone)
        : null;
}

/// <summary>
/// The drawn parts of the 订阅配额 card (Mac <c>ModernQuotaGauges.swift</c>):
/// the overview bar, the double ring, the round button face, the ring's
/// spinner. They only draw the numbers they are handed; no pace or colour
/// rule lives here. Geometry is in DIPs inside a canvas at a whole position,
/// so the hairline ticks keep their fractional sizes instead of being
/// rounded by layout. XAML has no conic gradient: gradient arcs are short
/// opaque segments, each blended over the surface it sits on.
/// </summary>
internal static class Gauges
{
    // -- colours (Mac ModernTheme.ink / accent / litColor) -----------------------------------

    public static Color? Accent(PaceTone tone) => tone switch
    {
        PaceTone.Amber => Theme.Amber,
        PaceTone.Red => Theme.Red,
        _ => null,
    };

    /// <summary>Lit parts brighten along their length: ink from 45 % to 90 % (95 % for solid lines), an accent from 62 % to 100 %.</summary>
    public static Color Lit(PaceTone tone, double progress, bool solid = false) =>
        Accent(tone) is Color accent ? WithAlpha(accent, 0.62 + 0.38 * progress) : Theme.InkC(0.45 + (solid ? 0.5 : 0.45) * progress);

    private static Color WithAlpha(Color c, double alpha) => Windows.UI.Color.FromArgb((byte)Math.Round(Math.Clamp(alpha, 0, 1) * 255), c.R, c.G, c.B);

    /// <summary>An alpha colour laid over an opaque one.</summary>
    public static Color Over(Color top, Color bottom)
    {
        double a = top.A / 255.0;
        return Windows.UI.Color.FromArgb(255,
            (byte)Math.Round(top.R * a + bottom.R * (1 - a)),
            (byte)Math.Round(top.G * a + bottom.G * (1 - a)),
            (byte)Math.Round(top.B * a + bottom.B * (1 - a)));
    }

    private static Path Fill(Geometry g, Color c) => new() { Data = g, Fill = new SolidColorBrush(c) };

    private static PathGeometry Polygon(params Point[] points)
    {
        var f = new PathFigure { StartPoint = points[0], IsClosed = true, IsFilled = true };
        for (int i = 1; i < points.Length; i++) f.Segments.Add(new LineSegment { Point = points[i] });
        return Geo.Geometry(f);
    }

    /// <summary>A rectangle (x, y, w, h) given relative to the centre at 12 o'clock, turned clockwise by <paramref name="turn"/> (0–1).</summary>
    private static PathGeometry Turned(double cx, double cy, double turn, double x, double y, double w, double h)
    {
        double a = turn * 2 * Math.PI, cos = Math.Cos(a), sin = Math.Sin(a);
        Point P(double px, double py) => new(cx + px * cos - py * sin, cy + px * sin + py * cos);
        return Polygon(P(x, y), P(x + w, y), P(x + w, y + h), P(x, y + h));
    }

    /// <summary>An annular sector between two clockwise angles (degrees from 12 o'clock).</summary>
    private static PathGeometry Sector(double cx, double cy, double rIn, double rOut, double fromDeg, double toDeg)
    {
        var f = new PathFigure { StartPoint = Geo.Polar(cx, cy, rOut, fromDeg), IsClosed = true, IsFilled = true };
        bool large = toDeg - fromDeg > 180;
        f.Segments.Add(new ArcSegment { Point = Geo.Polar(cx, cy, rOut, toDeg), Size = new Size(rOut, rOut), SweepDirection = SweepDirection.Clockwise, IsLargeArc = large });
        f.Segments.Add(new LineSegment { Point = Geo.Polar(cx, cy, rIn, toDeg) });
        f.Segments.Add(new ArcSegment { Point = Geo.Polar(cx, cy, rIn, fromDeg), Size = new Size(rIn, rIn), SweepDirection = SweepDirection.Counterclockwise, IsLargeArc = large });
        return Geo.Geometry(f);
    }

    private static Canvas Board(double w, double h) => new() { Width = w, Height = h, IsHitTestVisible = false };

    // -- overview bar (131a) -----------------------------------------------------------------

    public const int OverviewTicks = 61;
    public const double OverviewBarWidth = 242;

    /// <summary>
    /// The bar under an overview row: 5 DIP ticks for the longer window, the
    /// thin line under them for the shorter one, a small triangle (and a small
    /// mark on the line) where time says each should be. Without a shorter
    /// window the line is left out; without any window every tick is dim.
    /// </summary>
    public static Canvas OverviewBar(Gauge? outer, Gauge? inner, double width)
    {
        var c = Board(width, inner == null ? 10 : 16);
        double step = (width - 1.5) / (OverviewTicks - 1);
        for (int i = 0; i < OverviewTicks; i++)
        {
            double p = (i + 0.5) / OverviewTicks;
            var color = outer is { Remaining: > 0 } o && p <= o.Remaining ? Lit(o.Tone, p / o.Remaining) : Theme.InkC(0.13);
            c.Children.Add(Fill(new RectangleGeometry { Rect = new Rect(i * step, 5, 1.5, 5) }, color));
        }
        if (outer?.Expected is double expected)
        {
            double x = expected * width;
            c.Children.Add(Fill(Polygon(new(x - 2.5, 0), new(x + 2.5, 0), new(x, 4)), Theme.TextC(0.7)));
        }
        if (inner is not Gauge line) return c;
        c.Children.Add(Fill(new RectangleGeometry { Rect = new Rect(0, 12, width, 2.5) }.Rounded(1.25), Theme.InkC(0.11)));
        double lit = line.Remaining * width;
        if (lit > 0)
        {
            c.Children.Add(new Path
            {
                Data = new RectangleGeometry { Rect = new Rect(0, 12, lit, 2.5) }.Rounded(1.25),
                Fill = Theme.AbsoluteHorizontalGradient(Lit(line.Tone, 0, solid: true), Lit(line.Tone, 1, solid: true), 0, Math.Max(lit, 1)),
            });
        }
        if (line.Expected is double mark)
            c.Children.Add(Fill(new RectangleGeometry { Rect = new Rect(mark * width - 0.75, 11, 1.5, 5) }, Theme.TextC(0.7)));
        return c;
    }

    // -- double ring (131b–131d, 132m–132p) ---------------------------------------------------

    public const double RingSize = Theme.CardBodyHeight;

    /// <summary>The card surface behind the ring's middle (the card gradient about halfway down), for blending the arc.</summary>
    private static readonly Color RingSurface = Theme.Lerp(Theme.CardTop, Theme.CardBottom, 0.57);

    /// <summary>
    /// The 148 DIP ring of one subscription: 96 ticks for the longer window, a
    /// solid inner arc for the shorter one, a triangle outside the ticks and a
    /// small mark inside the arc where time says each should be. With one
    /// window only the ticks are drawn; with none the ticks are dim and the
    /// arc is an empty track (<paramref name="isEmpty"/>).
    /// </summary>
    public static Canvas Ring(Gauge? outer, Gauge? inner, bool isEmpty)
    {
        double s = RingSize, c0 = s / 2;
        var c = Board(s, s);
        for (int i = 0; i < 96; i++)
        {
            double p = (i + 0.5) / 96;
            var color = outer is { Remaining: > 0 } o && p <= o.Remaining ? Lit(o.Tone, p / o.Remaining) : Theme.InkC(0.13);
            c.Children.Add(Fill(Turned(c0, c0, p, -0.75, -67, 1.5, 6), color));
        }
        if (outer?.Expected is double expected)
        {
            double a = expected * 2 * Math.PI, cos = Math.Cos(a), sin = Math.Sin(a);
            Point P(double x, double y) => new(c0 + x * cos - y * sin, c0 + x * sin + y * cos);
            c.Children.Add(Fill(Polygon(P(-2.5, -74), P(2.5, -74), P(0, -70)), Theme.TextC(0.7)));
        }
        if (inner is Gauge arc)
        {
            var track = Over(Theme.InkC(0.09), RingSurface);
            c.Children.Add(Track(c0, 56, 6, Theme.InkC(0.09)));
            if (arc.Remaining > 0)
            {
                var start = arc.Tone == PaceTone.Neutral ? Theme.InkC(0.45) : Lit(arc.Tone, 0);
                var end = arc.Tone == PaceTone.Neutral ? Theme.InkC(0.95) : Lit(arc.Tone, 0.87);
                AddGradientArc(c, c0, c0, 56, 6, arc.Remaining, start, end, track, roundCaps: false);
            }
            if (arc.Expected is double mark)
                c.Children.Add(Fill(Turned(c0, c0, mark, -0.75, -51, 1.5, 6), Theme.TextC(0.7)));
        }
        else if (isEmpty)
        {
            c.Children.Add(Track(c0, 56, 6, Theme.InkC(0.09)));
        }
        return c;
    }

    private static Ellipse Track(double centre, double radius, double width, Color color)
    {
        var e = new Ellipse { Width = 2 * radius + width, Height = 2 * radius + width, Stroke = new SolidColorBrush(color), StrokeThickness = width };
        Canvas.SetLeft(e, centre - radius - width / 2);
        Canvas.SetTop(e, centre - radius - width / 2);
        return e;
    }

    /// <summary>
    /// An arc from 12 o'clock clockwise over <paramref name="fraction"/> of the
    /// circle whose colour runs from <paramref name="start"/> to
    /// <paramref name="end"/> along it (SwiftUI's conic gradient). Drawn as
    /// short opaque sectors, each blended over <paramref name="under"/> and
    /// overlapping its neighbour, so no seams show.
    /// </summary>
    private static void AddGradientArc(Canvas c, double cx, double cy, double radius, double width, double fraction,
        Color start, Color end, Func<double, Color> under, bool roundCaps)
    {
        double sweep = fraction * 360;
        int n = Math.Max(1, (int)Math.Ceiling(sweep / 2));
        double each = sweep / n;
        double rIn = radius - width / 2, rOut = radius + width / 2;
        for (int i = 0; i < n; i++)
        {
            double from = i * each, to = Math.Min(sweep, (i + 1) * each + (i < n - 1 ? 0.6 : 0));
            double mid = (from + Math.Min(sweep, (i + 1) * each)) / 2;
            var color = Over(Theme.Lerp(start, end, mid / sweep), under(mid));
            c.Children.Add(Fill(Sector(cx, cy, rIn, rOut, from, to), color));
        }
        if (!roundCaps) return;
        foreach (var (deg, col) in new[] { (0.0, start), (sweep, end) })
        {
            var p = Geo.Polar(cx, cy, radius, deg);
            var dot = new Ellipse { Width = width, Height = width, Fill = new SolidColorBrush(Over(col, under(deg))) };
            Canvas.SetLeft(dot, p.X - width / 2);
            Canvas.SetTop(dot, p.Y - width / 2);
            c.Children.Add(dot);
        }
    }

    private static void AddGradientArc(Canvas c, double cx, double cy, double radius, double width, double fraction,
        Color start, Color end, Color under, bool roundCaps) =>
        AddGradientArc(c, cx, cy, radius, width, fraction, start, end, _ => under, roundCaps);

    /// <summary>The ring's reading spinner (132m): a 22 DIP faint circle with a bright quarter, thinner than the small spinners.</summary>
    public static FrameworkElement RingSpinner()
    {
        const double size = 22, lw = 1.5, pad = 3.2;
        double d = size - 2 * pad, r = d / 2;
        var g = new Grid { Width = size, Height = size, IsHitTestVisible = false };
        var ring = new Ellipse { Width = d + lw, Height = d + lw, Stroke = new SolidColorBrush(Theme.TextC(0.14)), StrokeThickness = lw };
        ring.HorizontalAlignment = HorizontalAlignment.Center;
        ring.VerticalAlignment = VerticalAlignment.Center;
        g.Children.Add(ring);
        var arcCanvas = new Canvas { Width = size, Height = size };
        arcCanvas.Children.Add(Geo.Stroke(Geo.Arc(size / 2, size / 2, r, 0, 90), new SolidColorBrush(Theme.TextC(0.8)), lw));
        g.Children.Add(arcCanvas);
        Spin(arcCanvas, size);
        return g;
    }

    private static void Spin(FrameworkElement e, double size)
    {
        e.Loaded += (_, _) =>
        {
            var v = ElementCompositionPreview.GetElementVisual(e);
            v.CenterPoint = new Vector3((float)size / 2, (float)size / 2, 0);
            var linear = v.Compositor.CreateLinearEasingFunction();
            var spin = v.Compositor.CreateScalarKeyFrameAnimation();
            spin.InsertKeyFrame(0f, 0f, linear);
            spin.InsertKeyFrame(1f, 360f, linear);
            spin.Duration = TimeSpan.FromSeconds(0.9);
            spin.IterationBehavior = AnimationIterationBehavior.Forever;
            v.StartAnimation("RotationAngleInDegrees", spin);
        };
    }

    // -- round button (133) -------------------------------------------------------------------

    public const double ButtonSize = 30;
    private static readonly Color WellColor = Theme.Hex(0x121518);

    /// <summary>
    /// The well's colour at a point: <c>#121518</c> under the radial shade
    /// (clear to 78 % of 15 DIP from a centre 2 DIP low, black 70 % at the rim).
    /// </summary>
    private static Color WellAt(Point p)
    {
        double d = Math.Sqrt((p.X - 15) * (p.X - 15) + (p.Y - 17) * (p.Y - 17)) / 15;
        double shade = Math.Clamp((d - 0.78) / 0.22, 0, 1) * 0.7;
        return Over(Theme.Hex(0x000000, shade), WellColor);
    }

    /// <summary>
    /// The 30 DIP round button of one subscription: a dark well, the remaining
    /// arc of its tightest allowance, a raised face with the product's mark,
    /// lit when selected. A white dot at the top right marks a read that needs
    /// attention. A used-up allowance leaves only a red start point.
    /// </summary>
    public static Grid RoundButtonFace(Provider provider, double? remaining, bool selected, bool showsDot, bool hovered)
    {
        const double size = ButtonSize, radius = 13.25;
        var g = new Grid { Width = size, Height = size, IsHitTestVisible = false };

        // Well.
        g.Children.Add(new Ellipse { Width = size, Height = size, Fill = new SolidColorBrush(WellColor) });
        var shade = new RadialGradientBrush
        {
            MappingMode = BrushMappingMode.Absolute,
            Center = new Point(15, 17),
            GradientOrigin = new Point(15, 17),
            RadiusX = 15,
            RadiusY = 15,
        };
        shade.GradientStops.Add(new GradientStop { Color = Theme.Hex(0x000000, 0), Offset = 0.78 });
        shade.GradientStops.Add(new GradientStop { Color = Theme.Hex(0x000000, 0.7), Offset = 1 });
        g.Children.Add(new Ellipse { Width = size, Height = size, Fill = shade });
        var rim = new LinearGradientBrush { StartPoint = new Point(0, 0), EndPoint = new Point(0, 1) };
        rim.GradientStops.Add(new GradientStop { Color = Theme.Hex(0xFFFFFF, 0), Offset = 0.55 });
        rim.GradientStops.Add(new GradientStop { Color = Theme.Hex(0xFFFFFF, 0.1), Offset = 1 });
        g.Children.Add(new Ellipse { Width = size, Height = size, Stroke = rim, StrokeThickness = 0.5 });

        // Arc.
        var arc = Board(size, size);
        if (remaining is double rem)
        {
            double fraction = Math.Clamp(rem / 100, 0, 1);
            if (fraction > 0)
            {
                var end = selected ? Theme.InkC() : Theme.InkC(0.58);
                AddGradientArc(arc, 15, 15, radius, 2, fraction, Theme.InkC(0.2), end, deg => WellAt(Geo.Polar(15, 15, radius, deg)), roundCaps: true);
                if (selected)
                {
                    var tip = Geo.Polar(15, 15, radius, fraction * 360);
                    var glow = new RadialGradientBrush();
                    glow.GradientStops.Add(new GradientStop { Color = Theme.InkC(0.5), Offset = 0 });
                    glow.GradientStops.Add(new GradientStop { Color = Theme.InkC(0.28), Offset = 0.45 });
                    glow.GradientStops.Add(new GradientStop { Color = Theme.InkC(0), Offset = 1 });
                    var dot = new Ellipse { Width = 8, Height = 8, Fill = glow };
                    Canvas.SetLeft(dot, tip.X - 4);
                    Canvas.SetTop(dot, tip.Y - 4);
                    arc.Children.Add(dot);
                }
            }
            else
            {
                var red = new Ellipse { Width = 2, Height = 2, Fill = Theme.RedBrush };
                Canvas.SetLeft(red, 14);
                Canvas.SetTop(red, 15 - radius - 1);
                arc.Children.Add(red);
            }
        }
        g.Children.Add(arc);

        // Face: the soft shadow under it, the gradient disc, its edge light.
        var shadow = new RadialGradientBrush();
        shadow.GradientStops.Add(new GradientStop { Color = Theme.Hex(0x000000, 0.7), Offset = 0.72 });
        shadow.GradientStops.Add(new GradientStop { Color = Theme.Hex(0x000000, 0), Offset = 1 });
        g.Children.Add(new Ellipse
        {
            Width = 24, Height = 24, Fill = shadow,
            HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center,
            RenderTransform = new TranslateTransform { Y = 1.2 },
        });
        var (top, bottom) = selected ? (0x7C858Eu, 0x4F565Eu) : hovered ? (0x31363Cu, 0x25292Du) : (0x272B30u, 0x1C1F23u);
        g.Children.Add(new Ellipse
        {
            Width = 21, Height = 21, Fill = Theme.VerticalGradient(Theme.Hex(top), Theme.Hex(bottom)),
            HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center,
        });
        var edge = new LinearGradientBrush { StartPoint = new Point(0, 0), EndPoint = new Point(0, 1) };
        edge.GradientStops.Add(new GradientStop { Color = Theme.Hex(0xFFFFFF, selected ? 0.34 : 0.12), Offset = 0 });
        edge.GradientStops.Add(new GradientStop { Color = Theme.Hex(0xFFFFFF, 0), Offset = 0.4 });
        edge.GradientStops.Add(new GradientStop { Color = Theme.Hex(0x000000, 0), Offset = 0.62 });
        edge.GradientStops.Add(new GradientStop { Color = Theme.Hex(0x000000, selected ? 0.3 : 0.35), Offset = 1 });
        g.Children.Add(new Ellipse
        {
            Width = 21, Height = 21, Stroke = edge, StrokeThickness = 0.5,
            HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center,
        });

        // Mark: full text colour; a subscription with nothing to show and a
        // problem to look at dims it (132e), hovering brings it up.
        var markColor = selected ? Theme.Hex(0xFFFFFF) : showsDot && remaining == null && !hovered ? Theme.Hex(0x5E6369) : Theme.TextC();
        if (selected) g.Children.Add(ButtonGlyph(provider, Theme.Hex(0x000000, 0.4), 0.5));
        g.Children.Add(ButtonGlyph(provider, markColor, 0));

        if (showsDot)
        {
            var dot = Board(size, size);
            var outerDot = new Ellipse { Width = 9, Height = 9, Fill = new SolidColorBrush(Theme.Hex(0x1D2025)) };
            Canvas.SetLeft(outerDot, 21.5);
            Canvas.SetTop(outerDot, -0.5);
            var innerDot = new Ellipse { Width = 5, Height = 5, Fill = new SolidColorBrush(Theme.Hex(0xD8D5CF)) };
            Canvas.SetLeft(innerDot, 23.5);
            Canvas.SetTop(innerDot, 1.5);
            dot.Children.Add(outerDot);
            dot.Children.Add(innerDot);
            g.Children.Add(dot);
        }
        return g;
    }

    /// <summary>
    /// A mark inside a round button (Mac <c>ModernButtonGlyph</c>): the 24-unit
    /// path in a box sized to the mark's own visual weight, centred.
    /// </summary>
    private static FrameworkElement ButtonGlyph(Provider provider, Color color, double dy)
    {
        double side = provider switch
        {
            Provider.ClaudeCode => 15,
            Provider.Grok or Provider.Codex or Provider.KimiCode => 14,
            Provider.ZCode => 12,
            _ => 13,
        };
        var inner = new Canvas { Width = 24, Height = 24 };
        inner.Children.Add(new Path { Data = Geo.FromMarkup(ProviderMarks.Markup(provider)), Fill = new SolidColorBrush(color) });
        var box = new Viewbox { Width = side, Height = side, Child = inner, IsHitTestVisible = false };
        var holder = Board(ButtonSize, ButtonSize);
        Canvas.SetLeft(box, (ButtonSize - side) / 2);
        Canvas.SetTop(box, (ButtonSize - side) / 2 + dy);
        holder.Children.Add(box);
        return holder;
    }

    // -- the hanging block's shadow -----------------------------------------------------------

    /// <summary>
    /// A soft drop shadow under <paramref name="shape"/> (a rounded rectangle
    /// <paramref name="width"/> × <paramref name="height"/>), drawn by the
    /// compositor into <paramref name="host"/>, which sits behind it at the
    /// same place and size.
    /// </summary>
    public static void DropShadow(FrameworkElement host, Rectangle shape, double width, double height, float blur, float dy, float opacity)
    {
        shape.Loaded += (_, _) =>
        {
            try
            {
                var compositor = ElementCompositionPreview.GetElementVisual(host).Compositor;
                var shadow = compositor.CreateDropShadow();
                shadow.BlurRadius = blur;
                shadow.Offset = new Vector3(0, dy, 0);
                shadow.Color = Microsoft.UI.Colors.Black;
                shadow.Opacity = opacity;
                shadow.Mask = shape.GetAlphaMask();
                var sprite = compositor.CreateSpriteVisual();
                sprite.Size = new Vector2((float)width, (float)height);
                sprite.Shadow = shadow;
                ElementCompositionPreview.SetElementChildVisual(host, sprite);
            }
            catch (Exception)
            {
                // A shadow is decoration; the block reads fine without it.
            }
        };
    }
}

internal static class GeometryExtensions
{
    /// <summary>A rounded rectangle path (RectangleGeometry has no corner radius in WinUI).</summary>
    public static PathGeometry Rounded(this RectangleGeometry rect, double radius)
    {
        var r = rect.Rect;
        double k = Math.Min(radius, Math.Min(r.Width, r.Height) / 2);
        var f = new PathFigure { StartPoint = new Point(r.X + k, r.Y), IsClosed = true, IsFilled = true };
        void Arc(double x, double y) => f.Segments.Add(new ArcSegment { Point = new Point(x, y), Size = new Size(k, k), SweepDirection = SweepDirection.Clockwise });
        f.Segments.Add(new LineSegment { Point = new Point(r.Right - k, r.Y) });
        Arc(r.Right, r.Y + k);
        f.Segments.Add(new LineSegment { Point = new Point(r.Right, r.Bottom - k) });
        Arc(r.Right - k, r.Bottom);
        f.Segments.Add(new LineSegment { Point = new Point(r.X + k, r.Bottom) });
        Arc(r.X, r.Bottom - k);
        f.Segments.Add(new LineSegment { Point = new Point(r.X, r.Y + k) });
        Arc(r.X + k, r.Y);
        return Geo.Geometry(f);
    }
}
