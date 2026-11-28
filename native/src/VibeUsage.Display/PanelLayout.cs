using System.Drawing;

namespace VibeUsage.Display;

// Logical panel units. Callers supply measured bounds in a common coordinate space.
public static class PanelLayout
{
    public static double QuotaLabelWidth(IEnumerable<string> labels) =>
        Math.Max(28, Math.Ceiling(7 + labels.Select(s => s.EnumerateRunes().Sum(r => r.IsAscii ? 6.6 : 11)).DefaultIfEmpty().Max()));

    public const float PeakY = 18.5f;
    public static float CurveY(double value, double peak, float height)
    {
        float zero = height - 1.5f;
        return peak > 0 ? zero - (float)(value / peak) * (zero - PeakY) : zero;
    }

    public static PointF CostTooltip(PointF point, SizeF size, float plotWidth, RectangleF peakLabel)
    {
        float x = Math.Min(Math.Max(point.X - size.Width / 2, -12), plotWidth - size.Width);
        float above = point.Y - 10 - size.Height;
        var rect = new RectangleF(new PointF(x, above), size);
        return new(x, above < -6 || rect.IntersectsWith(peakLabel) ? point.Y + 10 : above);
    }

    public static RectangleF RemainderList(RectangleF row, int rows, RectangleF visible)
    {
        float wanted = rows > 10 ? 5 + 22 * 9.5f : 10 + 22 * rows;
        float below = visible.Bottom - row.Bottom - 2;
        float above = row.Top - visible.Top - 2;
        bool opensBelow = wanted <= below || wanted > above && below >= above;
        float room = Math.Max(below, above);
        float height = Math.Min(wanted, Math.Max(room, 38));
        return new(row.X - 9, opensBelow ? row.Bottom + 2 : row.Top - 2 - height, row.Width + 18, height);
    }
}
