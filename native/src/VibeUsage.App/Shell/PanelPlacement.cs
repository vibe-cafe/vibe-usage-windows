using System;

namespace VibeUsage.App.Shell;

public enum TaskbarEdge { Bottom, Top, Left, Right }

/// <summary>A rectangle in physical screen pixels.</summary>
public readonly record struct Box(double X, double Y, double W, double H)
{
    public double Right => X + W;
    public double Bottom => Y + H;
    public double Cx => X + W / 2;
    public double Cy => Y + H / 2;
    public bool Contains(double x, double y) => x >= X && x < Right && y >= Y && y < Bottom;
}

public readonly record struct Placement(double X, double Y, TaskbarEdge Edge, bool EdgeInferred);

/// <summary>
/// Where the panel goes, as a pure function (unit-tested): next to the
/// taskbar, centred on the icon along the taskbar, a margin away from it,
/// always inside the work area of the icon's monitor. With the icon on the
/// right of a bottom taskbar the clamp puts the panel in the bottom-right
/// corner, where Windows 11 opens its own quick settings and calendar. The
/// edge against the taskbar is computed from the work area, so when the
/// height changes (展开详情) that edge stays put and the panel grows away from
/// the taskbar.
/// </summary>
public static class PanelPlacement
{
    /// <summary>
    /// The taskbar is on the side where the work area is smaller than the
    /// monitor. An auto-hide taskbar leaves them equal; then the edge closest
    /// to the icon wins and the edge is "inferred".
    /// </summary>
    public static (TaskbarEdge Edge, bool Inferred) EdgeOf(Box monitor, Box work, Box? icon)
    {
        if (work.Y > monitor.Y) return (TaskbarEdge.Top, false);
        if (work.Bottom < monitor.Bottom) return (TaskbarEdge.Bottom, false);
        if (work.X > monitor.X) return (TaskbarEdge.Left, false);
        if (work.Right < monitor.Right) return (TaskbarEdge.Right, false);
        if (icon is not Box i) return (TaskbarEdge.Bottom, true);

        var best = (Distance: monitor.Bottom - i.Cy, Edge: TaskbarEdge.Bottom);
        foreach (var c in new[]
                 {
                     (i.Cy - monitor.Y, TaskbarEdge.Top),
                     (i.Cx - monitor.X, TaskbarEdge.Left),
                     (monitor.Right - i.Cx, TaskbarEdge.Right),
                 })
        {
            if (c.Item1 < best.Distance) best = c;
        }
        return (best.Edge, true);
    }

    /// <summary>Top-left corner for a w×h panel (physical px).</summary>
    public static Placement Place(Box monitor, Box work, Box? icon, double w, double h, double margin)
    {
        var (edge, inferred) = EdgeOf(monitor, work, icon);
        double ax = icon?.Cx ?? work.Right;
        double ay = icon?.Cy ?? work.Bottom;

        // Auto-hide taskbar: the work area includes the (shown) taskbar, so keep
        // clear of the icon instead. A normal taskbar sits outside the work area;
        // an icon in the overflow flyout sits inside it and must not push the
        // panel, so the icon's edge only counts when the edge was inferred.
        double IconEdge(double fallback, Func<Box, double> pick) =>
            inferred && icon is Box i ? pick(i) : fallback;

        double x, y;
        switch (edge)
        {
            case TaskbarEdge.Top:
                x = ax - w / 2;
                y = Math.Max(IconEdge(work.Y, b => b.Bottom), work.Y) + margin;
                break;
            case TaskbarEdge.Left:
                x = Math.Max(IconEdge(work.X, b => b.Right), work.X) + margin;
                y = ay - h / 2;
                break;
            case TaskbarEdge.Right:
                x = Math.Min(IconEdge(work.Right, b => b.X), work.Right) - margin - w;
                y = ay - h / 2;
                break;
            default:
                x = ax - w / 2;
                y = Math.Min(IconEdge(work.Bottom, b => b.Y), work.Bottom) - margin - h;
                break;
        }

        return new Placement(
            Clamp(x, work.X + margin, work.Right - margin - w),
            Clamp(y, work.Y + margin, work.Bottom - margin - h),
            edge,
            inferred);
    }

    /// <summary>
    /// Window height in px: the content at this scale plus what the visible
    /// frame adds, at most the work area minus both margins (then the middle
    /// scrolls, header and footer stay).
    /// </summary>
    public static double Height(double contentDip, double scale, double frameExtra, double workHeight, double margin) =>
        Math.Max(0, Math.Min(Math.Ceiling(contentDip * scale) + frameExtra, workHeight - 2 * margin));

    private static double Clamp(double v, double lo, double hi) => hi < lo ? lo : Math.Min(Math.Max(v, lo), hi);
}
