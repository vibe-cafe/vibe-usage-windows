using VibeUsage.PanelPoc.Shell;
using Xunit;

namespace VibeUsage.PanelPoc.Tests;

// Same cases as the Tauri prototype (poc/tray-popup, panel.rs) so both place the panel identically.
public class PlacementTests
{
    private static readonly Box M = new(0, 0, 1920, 1080);

    [Fact]
    public void BottomTaskbarOpensAboveTheIconInTheCorner()
    {
        var work = M with { H = 1032 };
        var icon = new Box(1700, 1040, 32, 40);
        var p = PanelPlacement.Place(M, work, icon, 380, 620, 12);
        Assert.Equal((TaskbarEdge.Bottom, false), (p.Edge, p.EdgeInferred));
        Assert.Equal(1032 - 12 - 620, p.Y);
        Assert.Equal(1920 - 12 - 380, p.X); // centred on the icon, clamped to the right margin
    }

    [Fact]
    public void IconNearTheMiddleCentresThePanelOnIt()
    {
        var work = M with { H = 1032 };
        var icon = new Box(900, 1040, 32, 40);
        var p = PanelPlacement.Place(M, work, icon, 380, 620, 12);
        Assert.Equal(916 - 190, p.X);
    }

    [Fact]
    public void OverflowFlyoutIconDoesNotPushThePanelUp()
    {
        var work = M with { H = 1032 };
        var iconInFlyout = new Box(1600, 900, 32, 32);
        var p = PanelPlacement.Place(M, work, iconInFlyout, 380, 620, 12);
        Assert.Equal(TaskbarEdge.Bottom, p.Edge);
        Assert.Equal(1032 - 12 - 620, p.Y);
    }

    [Fact]
    public void TopLeftAndRightTaskbars()
    {
        var top = M with { Y = 48, H = 1032 };
        var p = PanelPlacement.Place(M, top, new Box(1700, 4, 32, 40), 380, 620, 12);
        Assert.Equal((TaskbarEdge.Top, 60.0), (p.Edge, p.Y));

        var left = M with { X = 60, W = 1860 };
        p = PanelPlacement.Place(M, left, new Box(10, 900, 40, 32), 380, 620, 12);
        Assert.Equal((TaskbarEdge.Left, 72.0), (p.Edge, p.X));
        Assert.Equal(1080 - 12 - 620, p.Y);

        var right = M with { W = 1860 };
        p = PanelPlacement.Place(M, right, new Box(1870, 300, 40, 32), 380, 620, 12);
        Assert.Equal((TaskbarEdge.Right, 1860.0 - 12 - 380), (p.Edge, p.X));
        Assert.Equal(12, p.Y);
    }

    [Fact]
    public void AutoHideTaskbarKeepsClearOfTheIcon()
    {
        var icon = new Box(1700, 1032, 32, 48);
        var p = PanelPlacement.Place(M, M, icon, 380, 620, 12);
        Assert.Equal((TaskbarEdge.Bottom, true), (p.Edge, p.EdgeInferred));
        Assert.Equal(1032 - 12 - 620, p.Y);
    }

    [Fact]
    public void UnknownIconGoesToTheNotificationCorner()
    {
        var work = M with { H = 1032 };
        var p = PanelPlacement.Place(M, work, null, 380, 620, 12);
        Assert.Equal(TaskbarEdge.Bottom, p.Edge);
        Assert.Equal((1920.0 - 12 - 380, 1032.0 - 12 - 620), (p.X, p.Y));
    }

    [Fact]
    public void SecondaryMonitorWithNegativeCoordinatesAndScaling()
    {
        var mon = new Box(-2880, 0, 2880, 1620); // 150% monitor left of the primary, physical px
        var work = mon with { H = 1548 };
        var p = PanelPlacement.Place(mon, work, new Box(-300, 1560, 48, 60), 570, 930, 18);
        Assert.True(p.X >= mon.X + 18 && p.X + 570 <= mon.Right - 18);
        Assert.Equal(1548 - 18 - 930, p.Y);
    }

    [Fact]
    public void PanelLargerThanWorkAreaSticksToTheTopLeft()
    {
        var small = new Box(0, 0, 300, 500);
        var p = PanelPlacement.Place(small, small, null, 380, 620, 12);
        Assert.Equal((12.0, 12.0), (p.X, p.Y));
    }
}

public class TrayToggleGateTests
{
    [Fact]
    public void ClosedPanelOpensOnClick()
    {
        var g = new TrayToggleGate();
        Assert.Equal(TrayToggleGate.Decision.Open, g.OnSelect(10_000, panelVisible: false, keyboard: false));
    }

    [Fact]
    public void OpenPanelClosesOnClickWhenTheTaskbarDidNotStealActivation()
    {
        var g = new TrayToggleGate();
        Assert.Equal(TrayToggleGate.Decision.Close, g.OnSelect(10_000, panelVisible: true, keyboard: false));
    }

    [Fact]
    public void ClickThatClosedThePanelOnMouseDownDoesNotReopenIt()
    {
        var g = new TrayToggleGate();
        g.OnAutoClose(10_000, cursorOnIcon: true);            // mouse-down on the icon: panel deactivated
        Assert.Equal(TrayToggleGate.Decision.Swallow, g.OnSelect(10_120, false, false)); // mouse-up
        Assert.Equal(TrayToggleGate.Decision.Open, g.OnSelect(11_000, false, false));    // next click opens
    }

    [Fact]
    public void SlowPressAndHoldOnTheIconIsStillTreatedAsClose()
    {
        var g = new TrayToggleGate();
        g.OnAutoClose(10_000, cursorOnIcon: true);
        Assert.Equal(TrayToggleGate.Decision.Swallow, g.OnSelect(11_200, false, false));
    }

    [Fact]
    public void ButtonDownRightAfterAnAutoCloseMarksTheClickAsClose()
    {
        var g = new TrayToggleGate();
        g.OnAutoClose(10_000, cursorOnIcon: false); // icon rect unknown
        g.OnButtonDown(10_050, panelVisible: false);
        Assert.Equal(TrayToggleGate.Decision.Swallow, g.OnSelect(10_900, false, false));
    }

    [Fact]
    public void ClickingElsewhereThenTheIconLaterOpens()
    {
        var g = new TrayToggleGate();
        g.OnAutoClose(10_000, cursorOnIcon: false); // clicked the desktop
        g.OnButtonDown(12_000, panelVisible: false);
        Assert.Equal(TrayToggleGate.Decision.Open, g.OnSelect(12_100, false, false));
    }

    [Fact]
    public void VeryFastClickAfterAnyAutoCloseIsSwallowed()
    {
        var g = new TrayToggleGate();
        g.OnAutoClose(10_000, cursorOnIcon: false);
        Assert.Equal(TrayToggleGate.Decision.Swallow, g.OnSelect(10_200, false, false));
    }

    [Fact]
    public void StaleSwallowExpires()
    {
        var g = new TrayToggleGate();
        g.OnAutoClose(10_000, cursorOnIcon: true); // e.g. the icon was dragged, no click followed
        Assert.Equal(TrayToggleGate.Decision.Open, g.OnSelect(10_000 + TrayToggleGate.SwallowExpiryMs + 1, false, false));
    }

    [Fact]
    public void ResetAfterContextMenuForgetsPendingSwallow()
    {
        var g = new TrayToggleGate();
        g.OnAutoClose(10_000, cursorOnIcon: true); // right mouse-down on the icon closed the panel
        g.Reset();                                  // context menu shown
        Assert.Equal(TrayToggleGate.Decision.Open, g.OnSelect(10_500, false, false));
    }

    [Fact]
    public void DuplicateKeySelectIsIgnored()
    {
        var g = new TrayToggleGate();
        Assert.Equal(TrayToggleGate.Decision.Open, g.OnSelect(10_000, false, keyboard: true));
        Assert.Equal(TrayToggleGate.Decision.IgnoreRepeat, g.OnSelect(10_050, true, keyboard: true));
        Assert.Equal(TrayToggleGate.Decision.Close, g.OnSelect(10_600, true, keyboard: true));
    }

    [Fact]
    public void SixClicksOneSecondApartAlternateAndEndClosed()
    {
        // Checklist step C9, with the taskbar stealing activation on every mouse-down while open.
        var g = new TrayToggleGate();
        bool visible = false;
        for (int i = 0; i < 6; i++)
        {
            long down = 10_000 + i * 1000;
            if (visible)
            {
                visible = false;
                g.OnAutoClose(down, cursorOnIcon: true);
            }
            g.OnButtonDown(down + 5, visible);
            var d = g.OnSelect(down + 90, visible, false);
            if (d == TrayToggleGate.Decision.Open) visible = true;
            if (d == TrayToggleGate.Decision.Close) visible = false;
            Assert.Equal(i % 2 == 0, visible);
        }
        Assert.False(visible);
    }
}
