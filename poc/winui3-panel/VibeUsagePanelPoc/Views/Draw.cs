using System;
using System.Collections.Generic;
using System.Linq;
using System.Numerics;
using Microsoft.UI.Composition;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Hosting;
using Microsoft.UI.Xaml.Markup;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Shapes;
using VibeUsage.PanelPoc.Data;
using Windows.Foundation;
using Windows.UI;

namespace VibeUsage.PanelPoc.Views;

/// <summary>A meter as the gauges draw it (ModernGaugeValue): 0…1 left, 0…1 expected, tone.</summary>
public readonly record struct Gauge(double Remaining, double? Expected, Tone Tone)
{
    public static Gauge? Of(Meter? m) => m?.Remaining is double r
        ? new Gauge(Math.Clamp(r / 100, 0, 1), m.Expected is double e ? Math.Clamp(e / 100, 0, 1) : null, m.Tone)
        : null;
}

/// <summary>Geometry helpers. Angles are degrees clockwise from 12 o'clock, like the Mac code's rotations.</summary>
internal static partial class Geo
{
    public static Point Polar(double cx, double cy, double r, double deg)
    {
        double a = deg * Math.PI / 180;
        return new Point(cx + r * Math.Sin(a), cy - r * Math.Cos(a));
    }

    public static PathFigure Poly(bool closed, params double[] xy)
    {
        var f = new PathFigure { StartPoint = new Point(xy[0], xy[1]), IsClosed = closed, IsFilled = closed };
        for (int i = 2; i + 1 < xy.Length; i += 2) f.Segments.Add(new LineSegment { Point = new Point(xy[i], xy[i + 1]) });
        return f;
    }

    public static PathGeometry Geometry(params PathFigure[] figures)
    {
        var g = new PathGeometry();
        foreach (var f in figures) g.Figures.Add(f);
        return g;
    }

    public static PathGeometry Arc(double cx, double cy, double r, double startDeg, double endDeg)
    {
        var f = new PathFigure { StartPoint = Polar(cx, cy, r, startDeg), IsClosed = false, IsFilled = false };
        f.Segments.Add(new ArcSegment
        {
            Point = Polar(cx, cy, r, endDeg),
            Size = new Size(r, r),
            SweepDirection = SweepDirection.Clockwise,
            IsLargeArc = endDeg - startDeg > 180,
        });
        return Geometry(f);
    }

    public static Path Stroke(Geometry g, Brush brush, double thickness, PenLineCap cap = PenLineCap.Round) => new()
    {
        Data = g,
        Stroke = brush,
        StrokeThickness = thickness,
        StrokeStartLineCap = cap,
        StrokeEndLineCap = cap,
        StrokeLineJoin = PenLineJoin.Round,
    };

    public static Path Fill(Geometry g, Brush brush) => new() { Data = g, Fill = brush };

    public static void At(UIElement e, double x, double y)
    {
        Canvas.SetLeft(e, x);
        Canvas.SetTop(e, y);
    }

    /// <summary>
    /// An arc whose colour runs from <paramref name="start"/> to <paramref name="end"/> along its sweep
    /// (SwiftUI's conic / angular gradient), drawn as short segments.
    /// </summary>
    public static void ConicArc(Canvas canvas, double cx, double cy, double r, double sweepDeg, Color start, Color end,
        double thickness, bool roundEnds)
    {
        if (sweepDeg <= 0) return;
        int n = Math.Max(2, (int)Math.Ceiling(sweepDeg / 3));
        for (int k = 0; k < n; k++)
        {
            double a0 = k * sweepDeg / n;
            double a1 = Math.Min(sweepDeg, (k + 1) * sweepDeg / n + (k < n - 1 ? 0.6 : 0));
            var path = Stroke(Arc(cx, cy, r, a0, a1), new SolidColorBrush(Mt.Lerp(start, end, (k + 0.5) / n)), thickness, PenLineCap.Flat);
            if (roundEnds && k == 0) path.StrokeStartLineCap = PenLineCap.Round;
            if (roundEnds && k == n - 1) path.StrokeEndLineCap = PenLineCap.Round;
            canvas.Children.Add(path);
        }
    }
}

internal static partial class Icons
{
    private static Canvas Box(double w, double h, params UIElement[] children)
    {
        var c = new Canvas { Width = w, Height = h, VerticalAlignment = VerticalAlignment.Center, IsHitTestVisible = false };
        foreach (var ch in children) c.Children.Add(ch);
        return c;
    }

    /// <summary>SF "arrow.up.right" at 7 pt bold.</summary>
    public static FrameworkElement ArrowUpRight(Brush brush) => Box(6, 6,
        Geo.Stroke(Geo.Geometry(Geo.Poly(false, 0.8, 5.2, 5.2, 0.8), Geo.Poly(false, 1.7, 0.8, 5.2, 0.8, 5.2, 4.3)), brush, 1.15));

    /// <summary>SF "chevron.down" / "chevron.up" at 8 pt semibold.</summary>
    public static FrameworkElement Chevron(bool up, Brush brush, double width = 7)
    {
        double h = width * 0.55;
        var fig = up ? Geo.Poly(false, 0.6, h - 0.4, width / 2, 0.4, width - 0.6, h - 0.4)
                     : Geo.Poly(false, 0.6, 0.4, width / 2, h - 0.4, width - 0.6, 0.4);
        return Box(width, h, Geo.Stroke(Geo.Geometry(fig), brush, 1.25));
    }

    /// <summary>SF "line.3.horizontal.decrease" at 12 pt, in its 14 × 20 frame.</summary>
    public static FrameworkElement Filter(Brush brush) => Box(14, 20,
        Geo.Stroke(Geo.Geometry(Geo.Poly(false, 1.2, 6.4, 12.8, 6.4), Geo.Poly(false, 3.4, 10, 10.6, 10), Geo.Poly(false, 5.3, 13.6, 8.7, 13.6)), brush, 1.15));

    /// <summary>SF "arrow.up.arrow.down" at 9 pt semibold, in its 12 × 12 frame.</summary>
    public static FrameworkElement Sort(Brush brush) => Box(12, 12,
        Geo.Stroke(Geo.Geometry(
            Geo.Poly(false, 3.6, 10.6, 3.6, 1.4), Geo.Poly(false, 1.3, 3.7, 3.6, 1.4, 5.9, 3.7),
            Geo.Poly(false, 8.4, 1.4, 8.4, 10.6), Geo.Poly(false, 6.1, 8.3, 8.4, 10.6, 10.7, 8.3)), brush, 1.2));

    /// <summary>A Segoe Fluent Icons glyph (gear, power, circular arrows) where SF Symbols has the same idea.</summary>
    public static FontIcon Fluent(string glyph, double size, Brush brush) => new()
    {
        Glyph = glyph,
        FontSize = size,
        Foreground = brush,
        IsTextScaleFactorEnabled = false,
        VerticalAlignment = VerticalAlignment.Center,
        HorizontalAlignment = HorizontalAlignment.Center,
        IsHitTestVisible = false,
    };

    public const string Gear = "\uE713";
    public const string Power = "\uE7E8";
    public const string TwoArrowsCircle = "\uE895";
    public const string Clockwise = "\uE72C";

    /// <summary>A provider mark (24-unit Lobe path, even-odd) drawn <paramref name="drawn"/> wide, centred in a box.</summary>
    public static FrameworkElement Glyph(string data, double box, double drawn, Brush brush)
    {
        var path = (Path)XamlReader.Load(
            "<Path xmlns=\"http://schemas.microsoft.com/winfx/2006/xaml/presentation\" Data=\"" + data + "\" />");
        path.Fill = brush;
        var inner = new Grid { Width = 24, Height = 24 };
        inner.Children.Add(path);
        var g = new Grid { Width = box, Height = box, IsHitTestVisible = false };
        g.Children.Add(new Viewbox { Width = drawn, Height = drawn, Child = inner, HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center });
        return g;
    }

    /// <summary>The design's spinner: a faint ring with a bright quarter arc, turning every 0.9 s.</summary>
    public static FrameworkElement Spinner(double size, double faint = 0.18, double bright = 1, double? lineWidth = null, double? inset = null)
    {
        double lw = lineWidth ?? size * 2.4 / 24;
        double pad = inset ?? size * 3.5 / 24;
        double r = (size - 2 * pad - lw) / 2;
        var g = new Grid { Width = size, Height = size, IsHitTestVisible = false, VerticalAlignment = VerticalAlignment.Center };
        g.Children.Add(new Ellipse
        {
            Width = size - 2 * pad, Height = size - 2 * pad, Stroke = new SolidColorBrush(Mt.TextC(faint)), StrokeThickness = lw,
            HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center,
        });
        var arcCanvas = new Canvas { Width = size, Height = size };
        arcCanvas.Children.Add(Geo.Stroke(Geo.Arc(size / 2, size / 2, r, 0, 90), new SolidColorBrush(Mt.TextC(bright)), lw));
        g.Children.Add(arcCanvas);
        arcCanvas.Loaded += (_, _) =>
        {
            var v = ElementCompositionPreview.GetElementVisual(arcCanvas);
            v.CenterPoint = new Vector3((float)size / 2, (float)size / 2, 0);
            var spin = v.Compositor.CreateScalarKeyFrameAnimation();
            spin.InsertKeyFrame(0f, 0f, v.Compositor.CreateLinearEasingFunction());
            spin.InsertKeyFrame(1f, 360f, v.Compositor.CreateLinearEasingFunction());
            spin.Duration = TimeSpan.FromSeconds(0.9);
            spin.IterationBehavior = AnimationIterationBehavior.Forever;
            v.StartAnimation("RotationAngleInDegrees", spin);
        };
        return g;
    }
}

internal static partial class Gauges
{
    public static Color? Accent(Tone t) => t switch { Tone.Amber => Mt.Amber, Tone.Red => Mt.Red, _ => null };

    public static Brush Toned(Tone t, Brush neutral) => t switch { Tone.Amber => Mt.AmberBrush, Tone.Red => Mt.RedBrush, _ => neutral };

    /// <summary>Lit parts brighten along their length: ink 45→90 % (95 % solid), an accent 62→100 %.</summary>
    public static Color Lit(Tone t, double p, bool solid = false)
    {
        if (Accent(t) is Color a) return Microsoft.UI.ColorHelper.FromArgb((byte)Math.Round((0.62 + 0.38 * p) * 255), a.R, a.G, a.B);
        return Mt.InkC(0.45 + (solid ? 0.5 : 0.45) * p);
    }

    // -- overview bar (ModernOverviewBar) ------------------------------------------------------

    public const int OverviewTicks = 61;
    public const double OverviewWidth = 242;

    public static Canvas OverviewBar(Gauge? outer, Gauge? inner)
    {
        double w = OverviewWidth;
        var c = new Canvas { Width = w, Height = inner == null ? 10 : 16, IsHitTestVisible = false };
        double step = (w - 1.5) / (OverviewTicks - 1);
        var lit = new GeometryGroup();
        var off = new GeometryGroup();
        for (int i = 0; i < OverviewTicks; i++)
        {
            double p = (i + 0.5) / OverviewTicks;
            var rect = new RectangleGeometry { Rect = new Rect(i * step, 5, 1.5, 5) };
            if (outer is Gauge o && o.Remaining > 0 && p <= o.Remaining) lit.Children.Add(rect);
            else off.Children.Add(rect);
        }
        c.Children.Add(Geo.Fill(off, new SolidColorBrush(Mt.InkC(0.13))));
        if (outer is Gauge og && lit.Children.Count > 0)
            c.Children.Add(Geo.Fill(lit, Mt.AbsoluteHorizontalGradient(Lit(og.Tone, 0), Lit(og.Tone, 1), 0, og.Remaining * w)));
        if (outer?.Expected is double e)
        {
            double x = e * w;
            c.Children.Add(Geo.Fill(Geo.Geometry(Geo.Poly(true, x - 2.5, 0, x + 2.5, 0, x, 4)), new SolidColorBrush(Mt.TextC(0.7))));
        }
        if (inner is not Gauge ig) return c;
        c.Children.Add(Geo.Fill(new RectangleGeometry { Rect = new Rect(0, 12, w, 2.5) }, new SolidColorBrush(Mt.InkC(0.11))));
        double litWidth = ig.Remaining * w;
        if (litWidth > 0)
        {
            var bar = new Rectangle
            {
                Width = litWidth, Height = 2.5, RadiusX = 1.25, RadiusY = 1.25,
                Fill = Mt.AbsoluteHorizontalGradient(Lit(ig.Tone, 0, true), Lit(ig.Tone, 1, true), 0, litWidth),
            };
            Geo.At(bar, 0, 12);
            c.Children.Add(bar);
        }
        if (ig.Expected is double ie)
            c.Children.Add(Geo.Fill(new RectangleGeometry { Rect = new Rect(ie * w - 0.75, 11, 1.5, 5) }, new SolidColorBrush(Mt.TextC(0.7))));
        return c;
    }

    // -- double ring (ModernQuotaRing) ---------------------------------------------------------

    public static Grid QuotaRing(Gauge? outer, Gauge? inner, bool isEmpty, UIElement? center)
    {
        const double size = Mt.CardBodyHeight;
        const double cx = size / 2, cy = size / 2;
        var c = new Canvas { Width = size, Height = size, IsHitTestVisible = false };
        for (int i = 0; i < 96; i++)
        {
            double p = (i + 0.5) / 96;
            Color color = outer is Gauge o && o.Remaining > 0 && p <= o.Remaining ? Lit(o.Tone, p / o.Remaining) : Mt.InkC(0.13);
            var a = Geo.Polar(cx, cy, 61, p * 360);
            var b = Geo.Polar(cx, cy, 67, p * 360);
            c.Children.Add(new Line { X1 = a.X, Y1 = a.Y, X2 = b.X, Y2 = b.Y, Stroke = new SolidColorBrush(color), StrokeThickness = 1.5 });
        }
        if (outer?.Expected is double e)
        {
            var p1 = Geo.Polar(cx, cy, 74, e * 360 - 2.5 / 74 * 180 / Math.PI);
            var p2 = Geo.Polar(cx, cy, 74, e * 360 + 2.5 / 74 * 180 / Math.PI);
            var p3 = Geo.Polar(cx, cy, 70, e * 360);
            c.Children.Add(Geo.Fill(Geo.Geometry(Geo.Poly(true, p1.X, p1.Y, p2.X, p2.Y, p3.X, p3.Y)), new SolidColorBrush(Mt.TextC(0.7))));
        }
        if (inner != null || isEmpty)
        {
            var track = new Ellipse { Width = 118, Height = 118, StrokeThickness = 6, Stroke = new SolidColorBrush(Mt.InkC(0.09)) };
            Geo.At(track, cx - 59, cy - 59);
            c.Children.Add(track);
        }
        if (inner is Gauge g)
        {
            if (g.Remaining > 0)
            {
                Color start = g.Tone == Tone.Neutral ? Mt.InkC(0.45) : Lit(g.Tone, 0);
                Color end = g.Tone == Tone.Neutral ? Mt.InkC(0.95) : Lit(g.Tone, 0.87);
                Geo.ConicArc(c, cx, cy, 56, g.Remaining * 360, start, end, 6, false);
            }
            if (g.Expected is double ie)
            {
                var a = Geo.Polar(cx, cy, 45, ie * 360);
                var b = Geo.Polar(cx, cy, 51, ie * 360);
                c.Children.Add(new Line { X1 = a.X, Y1 = a.Y, X2 = b.X, Y2 = b.Y, Stroke = new SolidColorBrush(Mt.TextC(0.7)), StrokeThickness = 1.5 });
            }
        }
        var host = new Grid { Width = size, Height = size };
        host.Children.Add(c);
        if (center != null) host.Children.Add(center);
        return host;
    }

    // -- round button (ModernRoundButtonFace) --------------------------------------------------

    public const double ButtonSize = 30;

    public static Grid RoundButtonFace(Subscription s, bool selected, bool hovered)
    {
        const double size = ButtonSize, c = size / 2;
        var g = new Grid { Width = size, Height = size, IsHitTestVisible = false };
        var canvas = new Canvas { Width = size, Height = size };
        // Well: dark disc, darker towards its rim, a faint light rim at the bottom.
        canvas.Children.Add(new Ellipse { Width = size, Height = size, Fill = new SolidColorBrush(Mt.Hex(0x121518)) });
        var rimShade = new Ellipse { Width = size - 2, Height = size - 2, Stroke = new SolidColorBrush(Mt.Hex(0x000000, 0.35)), StrokeThickness = 2.4 };
        Geo.At(rimShade, 1, 1.4);
        canvas.Children.Add(rimShade);
        var rimLight = new Ellipse
        {
            Width = size - 0.5, Height = size - 0.5, StrokeThickness = 0.5,
            Stroke = TwoStopVertical(Mt.Hex(0xFFFFFF, 0), 0.55, Mt.Hex(0xFFFFFF, 0.1), 1),
        };
        Geo.At(rimLight, 0.25, 0.25);
        canvas.Children.Add(rimLight);

        // Arc of the tightest allowance.
        const double radius = 13.25;
        double fraction = Math.Clamp(s.ButtonRemaining / 100, 0, 1);
        if (fraction > 0)
        {
            Color end = selected ? Mt.InkC(1) : Mt.InkC(0.58);
            Geo.ConicArc(canvas, c, c, radius, fraction * 360, Mt.InkC(0.2), end, 2, true);
            if (selected)
            {
                var tip = Geo.Polar(c, c, radius, fraction * 360);
                var glow = new Ellipse { Width = 3.6, Height = 3.6, Fill = new SolidColorBrush(Mt.InkC(0.55)) };
                Geo.At(glow, tip.X - 1.8, tip.Y - 1.8);
                canvas.Children.Add(glow);
            }
        }
        else
        {
            var dot = new Ellipse { Width = 2, Height = 2, Fill = Mt.RedBrush };
            Geo.At(dot, c - 1, c - radius - 1);
            canvas.Children.Add(dot);
        }

        // Raised face.
        var shadow = new Ellipse { Width = 20.5, Height = 20.5, Fill = new SolidColorBrush(Mt.Hex(0x000000, 0.7)) };
        Geo.At(shadow, c - 10.25, c - 10.25 + 1.2);
        canvas.Children.Add(shadow);
        (uint top, uint bottom) = selected ? (0x7C858Eu, 0x4F565Eu) : hovered ? (0x31363Cu, 0x25292Du) : (0x272B30u, 0x1C1F23u);
        var face = new Ellipse
        {
            Width = 21, Height = 21,
            Fill = Mt.VerticalGradient(Mt.Hex(top), Mt.Hex(bottom)),
            StrokeThickness = 0.5,
            Stroke = FaceRim(selected ? 0.34 : 0.12, selected ? 0.3 : 0.35),
        };
        Geo.At(face, c - 10.5, c - 10.5);
        canvas.Children.Add(face);
        g.Children.Add(canvas);

        Brush glyphBrush = selected ? new SolidColorBrush(Mt.Hex(0xFFFFFF)) : Mt.Text;
        g.Children.Add(Icons.Glyph(s.Glyph, size, s.ButtonGlyphSide, glyphBrush));

        if (s.ShowsDot)
        {
            var outerDot = new Ellipse { Width = 9, Height = 9, Fill = new SolidColorBrush(Mt.Hex(0x1D2025)) };
            Geo.At(outerDot, 26 - 4.5, 4 - 4.5);
            canvas.Children.Add(outerDot);
            var innerDot = new Ellipse { Width = 5, Height = 5, Fill = new SolidColorBrush(Mt.Hex(0xD8D5CF)) };
            Geo.At(innerDot, 26 - 2.5, 4 - 2.5);
            canvas.Children.Add(innerDot);
        }
        return g;
    }

    private static LinearGradientBrush TwoStopVertical(Color a, double ao, Color b, double bo)
    {
        var br = new LinearGradientBrush { StartPoint = new Point(0, 0), EndPoint = new Point(0, 1) };
        br.GradientStops.Add(new GradientStop { Color = a, Offset = ao });
        br.GradientStops.Add(new GradientStop { Color = b, Offset = bo });
        return br;
    }

    private static LinearGradientBrush FaceRim(double top, double bottom)
    {
        var br = new LinearGradientBrush { StartPoint = new Point(0, 0), EndPoint = new Point(0, 1) };
        br.GradientStops.Add(new GradientStop { Color = Mt.Hex(0xFFFFFF, top), Offset = 0 });
        br.GradientStops.Add(new GradientStop { Color = Mt.Hex(0xFFFFFF, 0), Offset = 0.4 });
        br.GradientStops.Add(new GradientStop { Color = Mt.Hex(0x000000, 0), Offset = 0.62 });
        br.GradientStops.Add(new GradientStop { Color = Mt.Hex(0x000000, bottom), Offset = 1 });
        return br;
    }

    // -- lamp (ModernLamp) ---------------------------------------------------------------------

    public static Grid Lamp(bool on)
    {
        var g = new Grid { Width = 12, Height = 12, IsHitTestVisible = false, VerticalAlignment = VerticalAlignment.Center };
        g.Children.Add(new Ellipse { Fill = new SolidColorBrush(Mt.Well) });
        g.Children.Add(new Ellipse { Stroke = new SolidColorBrush(Mt.Hex(0x000000, 0.6)), StrokeThickness = 1.5, Margin = new Thickness(0, 0.5, 0, 0) });
        if (on)
            g.Children.Add(new Ellipse { Width = 10, Height = 10, Fill = new SolidColorBrush(Mt.TextC(0.22)) });
        g.Children.Add(new Ellipse { Width = 6, Height = 6, Fill = new SolidColorBrush(on ? Mt.LampOn : Mt.LampOff) });
        return g;
    }

    // -- distribution ring (ModernDistributionRing) --------------------------------------------

    public const double DistributionSize = 88;
    private const int DistributionTicks = 72;

    public static Canvas DistributionRing(IReadOnlyList<(double Value, Color Color)> segments)
    {
        const double size = DistributionSize, cx = size / 2, cy = size / 2;
        var c = new Canvas { Width = size, Height = size, IsHitTestVisible = false };
        var owners = TickOwners(segments.Select(s => s.Value).ToArray(), DistributionTicks);
        for (int i = 0; i < DistributionTicks; i++)
        {
            double deg = (i + 0.5) / DistributionTicks * 360;
            var a = Geo.Polar(cx, cy, 34, deg);
            var b = Geo.Polar(cx, cy, 43, deg);
            Color color = owners[i] is int seg ? segments[seg].Color : segments.Count == 0 ? Mt.InkC(0.13) : Mt.InkC(0.07);
            c.Children.Add(new Line { X1 = a.X, Y1 = a.Y, X2 = b.X, Y2 = b.Y, Stroke = new SolidColorBrush(color), StrokeThickness = 1.7 });
        }
        return c;
    }

    /// <summary>Same rule as the Mac ring: every positive share at least one tick, largest remainders for the rest, the last tick of a run left as the gap.</summary>
    public static int?[] TickOwners(double[] values, int count)
    {
        double sum = values.Sum(v => Math.Max(v, 0));
        if (sum <= 0) return new int?[count];
        var exact = values.Select(v => Math.Max(v, 0) / sum * count).ToArray();
        var runs = exact.Select(e => e > 0 ? Math.Max(1, (int)Math.Floor(e)) : 0).ToArray();
        int left = count - runs.Sum();
        var byRemainder = Enumerable.Range(0, exact.Length).Where(i => exact[i] > 0)
            .OrderByDescending(i => exact[i] - Math.Floor(exact[i])).ToArray();
        while (left > 0 && byRemainder.Length > 0)
        {
            foreach (int i in byRemainder)
            {
                if (left <= 0) break;
                runs[i]++;
                left--;
            }
        }
        while (left < 0)
        {
            int longest = Array.IndexOf(runs, runs.Max());
            runs[longest]--;
            left++;
        }
        bool separated = runs.Count(r => r > 0) > 1;
        var owners = new List<int?>();
        for (int seg = 0; seg < runs.Length; seg++)
        {
            int run = runs[seg];
            if (run <= 0) continue;
            int lit = separated && run > 1 ? run - 1 : run;
            for (int k = 0; k < lit; k++) owners.Add(seg);
            for (int k = 0; k < run - lit; k++) owners.Add(null);
        }
        while (owners.Count < count) owners.Add(null);
        return owners.Take(count).ToArray();
    }
}
