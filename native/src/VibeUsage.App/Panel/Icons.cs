using System;
using System.Collections.Generic;
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

/// <summary>Geometry helpers. Angles are degrees clockwise from 12 o'clock, like the Mac code's rotations.</summary>
internal static class Geo
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

    /// <summary>Path markup read by <see cref="PathMarkup"/> as a XAML geometry (a new one each call: geometries can't be shared).</summary>
    public static PathGeometry FromMarkup(string markup)
    {
        var data = PathMarkup.Parse(markup);
        var g = new PathGeometry { FillRule = data.EvenOdd ? FillRule.EvenOdd : FillRule.Nonzero };
        foreach (var figure in data.Figures)
        {
            var f = new PathFigure { StartPoint = P(figure.Start), IsClosed = figure.Closed, IsFilled = true };
            foreach (var segment in figure.Segments)
            {
                f.Segments.Add(segment switch
                {
                    PathSegmentData.Cubic c => new BezierSegment { Point1 = P(c.Control1), Point2 = P(c.Control2), Point3 = P(c.To) },
                    PathSegmentData.Arc a => new ArcSegment
                    {
                        Point = P(a.To),
                        Size = new Size(a.RadiusX, a.RadiusY),
                        RotationAngle = a.Rotation,
                        IsLargeArc = a.LargeArc,
                        SweepDirection = a.Clockwise ? SweepDirection.Clockwise : SweepDirection.Counterclockwise,
                    },
                    _ => new LineSegment { Point = P(segment.To) },
                });
            }
            g.Figures.Add(f);
        }
        return g;
    }

    private static Point P(PathPoint p) => new(p.X, p.Y);
}

/// <summary>The panel's small marks, drawn after the Mac's SF Symbols at the same sizes.</summary>
internal static class Icons
{
    private static Canvas Box(double w, double h, params UIElement[] children)
    {
        var c = new Canvas { Width = w, Height = h, VerticalAlignment = VerticalAlignment.Center, IsHitTestVisible = false };
        foreach (var ch in children) c.Children.Add(ch);
        return c;
    }

    /// <summary>SF "arrow.up.right" at 7 pt bold.</summary>
    public static Canvas ArrowUpRight(Brush brush) => Box(6, 6,
        Geo.Stroke(Geo.Geometry(Geo.Poly(false, 0.8, 5.2, 5.2, 0.8), Geo.Poly(false, 1.7, 0.8, 5.2, 0.8, 5.2, 4.3)), brush, 1.15));

    /// <summary>SF "chevron.down" / "chevron.up" at 8 pt semibold.</summary>
    public static Canvas Chevron(bool up, Brush brush, double width = 7)
    {
        double h = width * 0.55;
        var fig = up ? Geo.Poly(false, 0.6, h - 0.4, width / 2, 0.4, width - 0.6, h - 0.4)
                     : Geo.Poly(false, 0.6, 0.4, width / 2, h - 0.4, width - 0.6, 0.4);
        return Box(width, h, Geo.Stroke(Geo.Geometry(fig), brush, 1.25));
    }

    /// <summary>SF "arrow.up.arrow.down" at 9 pt semibold, in its 12 × 12 frame.</summary>
    public static Canvas Sort(Brush brush) => Box(12, 12,
        Geo.Stroke(Geo.Geometry(
            Geo.Poly(false, 3.6, 10.6, 3.6, 1.4), Geo.Poly(false, 1.3, 3.7, 3.6, 1.4, 5.9, 3.7),
            Geo.Poly(false, 8.4, 1.4, 8.4, 10.6), Geo.Poly(false, 6.1, 8.3, 8.4, 10.6, 10.7, 8.3)), brush, 1.2));

    /// <summary>SF "exclamationmark.circle" at 9.5 pt medium, in its 12 × 12 frame.</summary>
    public static Canvas Alert(Brush brush)
    {
        var ring = new Ellipse { Width = 10.6, Height = 10.6, Stroke = brush, StrokeThickness = 1.1 };
        Canvas.SetLeft(ring, 0.7);
        Canvas.SetTop(ring, 0.7);
        var dot = new Ellipse { Width = 1.4, Height = 1.4, Fill = brush };
        Canvas.SetLeft(dot, 5.3);
        Canvas.SetTop(dot, 8.0);
        return Box(12, 12, ring, Geo.Stroke(Geo.Geometry(Geo.Poly(false, 6, 3.3, 6, 6.6)), brush, 1.2), dot);
    }

    /// <summary>A Segoe Fluent Icons glyph (gear, power) where SF Symbols has the same idea.</summary>
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

    private static readonly Dictionary<Provider, string> MarkCache = [];

    /// <summary>A provider mark (24-unit Lobe path, even-odd) at the Mac's size for a <paramref name="slot"/>-wide box.</summary>
    public static FrameworkElement Mark(Provider provider, double slot, Brush brush)
    {
        if (!MarkCache.TryGetValue(provider, out var markup)) MarkCache[provider] = markup = ProviderMarks.Markup(provider);
        double drawn = ProviderMarks.DrawnSize(provider, slot);
        var holder = new Grid { Width = slot, Height = slot, IsHitTestVisible = false };
        if (markup.Length == 0) return holder;
        var inner = new Canvas { Width = 24, Height = 24 };
        inner.Children.Add(new Path { Data = Geo.FromMarkup(markup), Fill = brush });
        holder.Children.Add(new Viewbox
        {
            Width = drawn,
            Height = drawn,
            Child = inner,
            HorizontalAlignment = HorizontalAlignment.Center,
            VerticalAlignment = VerticalAlignment.Center,
        });
        return holder;
    }

    /// <summary>The design's spinner: a faint ring with a bright quarter arc, turning every 0.9 s.</summary>
    public static FrameworkElement Spinner(double size)
    {
        double lw = size * 2.4 / 24;
        double pad = size * 3.5 / 24;
        double r = (size - 2 * pad - lw) / 2;
        var g = new Grid { Width = size, Height = size, IsHitTestVisible = false, VerticalAlignment = VerticalAlignment.Center };
        g.Children.Add(new Ellipse
        {
            Width = size - 2 * pad - lw,
            Height = size - 2 * pad - lw,
            Stroke = new SolidColorBrush(Theme.TextC(0.18)),
            StrokeThickness = lw,
            HorizontalAlignment = HorizontalAlignment.Center,
            VerticalAlignment = VerticalAlignment.Center,
        });
        var arcCanvas = new Canvas { Width = size, Height = size };
        arcCanvas.Children.Add(Geo.Stroke(Geo.Arc(size / 2, size / 2, r, 0, 90), new SolidColorBrush(Theme.TextC()), lw));
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

    /// <summary>The 概览 lamp (Mac <c>ModernLamp</c>): a dark well with a glowing dot when 概览 is selected.</summary>
    public static Grid Lamp(bool on)
    {
        var g = new Grid { Width = 12, Height = 12, IsHitTestVisible = false, VerticalAlignment = VerticalAlignment.Center };
        g.Children.Add(new Ellipse { Width = 12, Height = 12, Fill = new SolidColorBrush(Theme.Well) });
        g.Children.Add(new Ellipse
        {
            Width = 11, Height = 11,
            Stroke = Theme.AbsoluteVerticalGradient(Theme.Hex(0x000000, 0.8), Theme.Hex(0x000000, 0), 0, 6),
            StrokeThickness = 1.5,
            HorizontalAlignment = HorizontalAlignment.Center,
            VerticalAlignment = VerticalAlignment.Center,
        });
        if (on)
        {
            g.Children.Add(new Ellipse
            {
                Width = 9, Height = 9, Fill = new SolidColorBrush(Theme.TextC(0.22)),
                HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center,
            });
        }
        g.Children.Add(new Ellipse
        {
            Width = 6, Height = 6,
            Fill = new SolidColorBrush(on ? Theme.LampOn : Theme.LampOff),
            HorizontalAlignment = HorizontalAlignment.Center,
            VerticalAlignment = VerticalAlignment.Center,
        });
        return g;
    }
}
