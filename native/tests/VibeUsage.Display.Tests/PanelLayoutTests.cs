using System.Drawing;
using static VibeUsage.Display.Tests.UsageDisplayRulesTests;

namespace VibeUsage.Display.Tests;

public class PanelLayoutTests
{
    [Fact]
    public void labelColumnWidensOnlyForNamedAllowances()
    {
        Assert.Equal(28, PanelLayout.QuotaLabelWidth(["5h", "7d"]));
        Assert.Equal(40, PanelLayout.QuotaLabelWidth(["5h", "7d", "Fable"]));
        Assert.Equal(29, PanelLayout.QuotaLabelWidth(["5h", "额外"]));
    }

    [Fact]
    public void aPeakAtTheLeftEdgeStaysUnderThePeakLabel()
    {
        var s = Snap([B("2026-11-26T16:00Z", cost: 134), B("2026-11-27T07:00Z", cost: 39)]);
        var chart = new RecentUsageChart(s, Hero.Cost, new());
        Assert.True(chart.HoverPoints[0].X < 0.05);
        Assert.Equal("$134", chart.PeakLabel);
        Assert.Equal(PanelLayout.PeakY, PanelLayout.CurveY(chart.HoverPoints[0].Value, chart.Peak, 128));
        Assert.Equal(126.5, PanelLayout.CurveY(0, 0, 128));
        Assert.All(chart.Curve, p => Assert.InRange(PanelLayout.CurveY(p.Value, chart.Peak, 128), 18.5f, 126.5f));
        Assert.True(PanelLayout.PeakY - 4 >= 14);
    }

    [Fact]
    public void theCostHoverBlockStepsUnderThePointRatherThanCoverThePeakLabel()
    {
        var label = new RectangleF(0, 2, 24, 12);
        var size = new SizeF(104, 22);
        PointF Origin(float x, float y) => PanelLayout.CostTooltip(new(x, y), size, 304, label);
        float y = PanelLayout.CurveY(60, 67, 128);
        Assert.Equal(y + 10, Origin(25, y).Y);
        Assert.False(new RectangleF(Origin(25, y), size).IntersectsWith(label));
        Assert.Equal(y - 32, Origin(220, y).Y);
        Assert.Equal(58, Origin(25, 90).Y);
        Assert.Equal(28.5, Origin(220, 18.5f).Y);
    }

    [Fact]
    public void remainderListOpensUnderItsRowAndAboveItAtTheBottomOfTheGrid()
    {
        var grid = new RectangleF(0, 0, 496, 512);
        var row = new RectangleF(16, 214, 212, 22);
        Assert.Equal(new(7, 238, 230, 76), PanelLayout.RemainderList(row, 3, grid));
        Assert.Equal(new(259, 396, 230, 76), PanelLayout.RemainderList(new(268, 474, 212, 22), 3, grid));
        Assert.Equal(new(7, 238, 230, 214), PanelLayout.RemainderList(row, 12, grid));
    }

    [Fact]
    public void remainderListStaysInTheVisiblePartOfAScrolledPanel()
    {
        var row = new RectangleF(16, 214, 212, 22);
        Assert.Equal(new(7, 136, 230, 76), PanelLayout.RemainderList(row, 3, new(0, -240, 496, 480)));
        Assert.Equal(new(7, 150, 230, 62), PanelLayout.RemainderList(row, 3, new(0, 150, 496, 120)));
        Assert.Equal(38, PanelLayout.RemainderList(row, 3, new(0, 205, 496, 40)).Height);
    }
}
