using System;
using System.Collections.Generic;
using System.Linq;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Shapes;
using VibeUsage.PanelPoc.Data;
using Windows.Foundation;

namespace VibeUsage.PanelPoc.Views;

/// <summary>
/// 近期消耗's curve (RecentUsagePlot / RecentUsageCurve / RecentUsagePlotLayout):
/// the peak's label in its own band at the top, the dashed peak line under it,
/// a monotone cubic through the points with a gradient under it, the end dot,
/// two ticks, the axis below. Hovering snaps to the nearest point.
/// </summary>
internal static class UsagePlot
{
    public const double AxisHeight = 18;
    public const double Spacing = 2;
    public const double CurveHeight = Mt.CardBodyHeight - Spacing - AxisHeight; // 128
    private const double LabelTop = 2, LabelHeight = 12, MarkerRadius = 4;
    private const double PeakY = LabelTop + LabelHeight + MarkerRadius + 0.5; // 18.5

    private static double Y(double fraction, double height)
    {
        double zero = height - 1.5;
        return zero - fraction * (zero - PeakY);
    }

    public static FrameworkElement Build(Period period, double share, double width)
    {
        double h = CurveHeight;
        double peak = period.Peak * share;
        var root = new Grid { Width = width, Height = Mt.CardBodyHeight };
        var plot = new Canvas { Width = width, Height = h, Background = Mt.Transparent, VerticalAlignment = VerticalAlignment.Top };
        root.Children.Add(plot);

        var points = period.Curve.Select(p => new Point(p.X * width, Y(p.V, h))).ToList();

        // Baseline.
        plot.Children.Add(new Line { X1 = 0, Y1 = h - 0.5, X2 = width, Y2 = h - 0.5, Stroke = new SolidColorBrush(Mt.InkC(0.14)), StrokeThickness = 1 });

        if (points.Count > 1)
        {
            double top = points.Min(p => p.Y);
            var fill = MonotoneFigure(points);
            fill.Segments.Add(new LineSegment { Point = new Point(points[^1].X, h) });
            fill.Segments.Add(new LineSegment { Point = new Point(points[0].X, h) });
            fill.IsClosed = true;
            fill.IsFilled = true;
            plot.Children.Add(Geo.Fill(Geo.Geometry(fill), Mt.AbsoluteVerticalGradient(Mt.InkC(0.28), Mt.InkC(0.02), top, h)));

            var line = MonotoneFigure(points);
            line.IsFilled = false;
            plot.Children.Add(Geo.Stroke(Geo.Geometry(line), new SolidColorBrush(Mt.InkC(0.92)), 1.5));

            var dot = new Ellipse { Width = 6, Height = 6, Fill = new SolidColorBrush(Mt.InkC(1)) };
            Geo.At(dot, width - 3.5 - 3, points[^1].Y - 3);
            plot.Children.Add(dot);

            // The peak's label and the dashed line under it.
            var label = Mt.T(FakeData.Peak(peak), 11, Face.Mono, Wt.Regular, Mt.Secondary);
            var labelBox = Mt.AtBaseline(label, LabelTop + Mt.BaselineIn(LabelHeight, (11, Face.Mono)));
            plot.Children.Add(labelBox);
            plot.Children.Add(new Line
            {
                X1 = 0, X2 = width, Y1 = PeakY, Y2 = PeakY, Stroke = new SolidColorBrush(Mt.TextC(0.12)), StrokeThickness = 1,
                StrokeDashArray = new DoubleCollection { 3, 3 },
            });

            foreach (var tick in period.Ticks)
            {
                var r = new Rectangle { Width = 1, Height = 4, Fill = new SolidColorBrush(Mt.TextC(0.2)) };
                Geo.At(r, Math.Round(tick.X * width), h - 4);
                plot.Children.Add(r);
            }

            AttachHover(plot, period, share, points, width, h);
        }

        var axis = Axis(period, width);
        axis.VerticalAlignment = VerticalAlignment.Top;
        axis.Margin = new Thickness(0, h + Spacing, 0, 0);
        root.Children.Add(axis);
        return root;
    }

    private static Grid Axis(Period period, double width)
    {
        var g = new Grid { Width = width, Height = AxisHeight };
        double baseline = Mt.BaselineIn(AxisHeight, (11, Face.Mono));
        g.Children.Add(Mt.AtBaseline(Mt.Words(period.StartLabel, 11, 4.4, Face.Mono, Wt.Regular, Mt.Secondary), baseline));
        var ticks = new Canvas { Width = width, Height = AxisHeight };
        foreach (var tick in period.Ticks)
        {
            var box = Mt.AtBaseline(Mt.T(tick.Label, 11, Face.Mono, Wt.Regular, Mt.Secondary), baseline, HorizontalAlignment.Center);
            box.Width = 80;
            Geo.At(box, tick.X * width - 40, 0);
            ticks.Children.Add(box);
        }
        g.Children.Add(ticks);
        bool present = period.EndLabel.Length > 0 && !char.IsDigit(period.EndLabel[0]);
        var end = present ? Mt.T(period.EndLabel, 11, Face.Text, Wt.Medium, Mt.Text) : Mt.T(period.EndLabel, 11, Face.Mono, Wt.Regular, Mt.Secondary);
        g.Children.Add(Mt.AtBaseline(end, present ? Mt.BaselineIn(AxisHeight, (11, Face.Text)) : baseline, HorizontalAlignment.Right));
        return g;
    }

    private static void AttachHover(Canvas plot, Period period, double share, List<Point> points, double width, double h)
    {
        var marker = new Canvas { Visibility = Visibility.Collapsed, IsHitTestVisible = false };
        var crosshair = new Rectangle { Width = 1, Fill = new SolidColorBrush(Mt.TextC(0.3)) };
        var ring = new Ellipse
        {
            Width = 2 * MarkerRadius, Height = 2 * MarkerRadius, Fill = new SolidColorBrush(Mt.MarkerFill),
            Stroke = new SolidColorBrush(Mt.InkC(1)), StrokeThickness = 1.5,
        };
        var text = Mt.Block();
        var block = Mt.RaisedBlock(text, 7, new Thickness(8, 4, 8, 5));
        marker.Children.Add(crosshair);
        marker.Children.Add(ring);
        marker.Children.Add(block);
        plot.Children.Add(marker);

        plot.PointerMoved += (_, e) =>
        {
            double x = e.GetCurrentPoint(plot).Position.X;
            int i = 0;
            for (int k = 1; k < points.Count; k++) if (Math.Abs(points[k].X - x) < Math.Abs(points[i].X - x)) i = k;
            var p = points[i];
            crosshair.Height = Math.Max(0, h - p.Y);
            Geo.At(crosshair, p.X - 0.5, p.Y);
            Geo.At(ring, p.X - MarkerRadius, p.Y - MarkerRadius);
            text.Inlines.Clear();
            string slot = i < period.SlotLabels.Count ? period.SlotLabels[i] : "";
            text.Inlines.Add(Mt.R(slot, 11, Face.Mono, Wt.Regular, Mt.Secondary));
            text.Inlines.Add(Mt.Gap(8));
            text.Inlines.Add(Mt.R(FakeData.Cost(period.Curve[i].V * period.Peak * share), 11, Face.Mono, Wt.Medium, Mt.Text));
            block.Measure(new Size(double.PositiveInfinity, double.PositiveInfinity));
            var size = block.DesiredSize;
            double bx = Math.Min(Math.Max(p.X - size.Width / 2, -12), width - size.Width);
            double above = p.Y - 10 - size.Height;
            Geo.At(block, bx, above < -6 ? p.Y + 10 : above);
            marker.Visibility = Visibility.Visible;
        };
        plot.PointerExited += (_, _) => marker.Visibility = Visibility.Collapsed;
    }

    /// <summary>Fritsch–Carlson monotone cubic (the design's pchip) as Bézier segments.</summary>
    private static PathFigure MonotoneFigure(IReadOnlyList<Point> pts)
    {
        var f = new PathFigure { StartPoint = pts[0], IsClosed = false };
        int n = pts.Count;
        if (n < 2) return f;
        var slopes = new double[n - 1];
        for (int i = 0; i < n - 1; i++)
        {
            double dx = pts[i + 1].X - pts[i].X;
            slopes[i] = dx == 0 ? 0 : (pts[i + 1].Y - pts[i].Y) / dx;
        }
        var t = new double[n];
        t[0] = slopes[0];
        t[n - 1] = slopes[n - 2];
        for (int i = 1; i < n - 1; i++)
        {
            double l = slopes[i - 1], r = slopes[i];
            if (l * r <= 0) { t[i] = 0; continue; }
            double h0 = pts[i].X - pts[i - 1].X, h1 = pts[i + 1].X - pts[i].X;
            double w0 = 2 * h1 + h0, w1 = h1 + 2 * h0;
            t[i] = (w0 + w1) / (w0 / l + w1 / r);
        }
        for (int i = 0; i < n - 1; i++)
        {
            var p0 = pts[i];
            var p1 = pts[i + 1];
            double hh = (p1.X - p0.X) / 3;
            f.Segments.Add(new BezierSegment
            {
                Point1 = new Point(p0.X + hh, p0.Y + t[i] * hh),
                Point2 = new Point(p1.X - hh, p1.Y - t[i + 1] * hh),
                Point3 = p1,
            });
        }
        return f;
    }
}
