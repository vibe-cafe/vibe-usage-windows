using System;
using System.Linq;
using VibeUsage.App.Shell;
using Xunit;

namespace VibeUsage.App.Tests;

public class OpenPolicyTests
{
    [Theory]
    [InlineData(OpenSource.TrayMouse, false)]
    [InlineData(OpenSource.Menu, false)]
    [InlineData(OpenSource.MenuKeyboard, true)]
    [InlineData(OpenSource.TrayKeyboard, true)]
    [InlineData(OpenSource.Relaunch, true)]
    [InlineData(OpenSource.Startup, true)]
    public void OnlyMouseOpensFromTheTrayLeaveTheKeyboardAlone(OpenSource source, bool activates)
    {
        // Decision 4: no global Esc hotkey; Esc works exactly when the panel has the keyboard.
        Assert.Equal(activates, OpenPolicy.Activates(source));
    }

    [Fact]
    public void KeyboardMenuOpenKeepsTheForegroundForThePanel()
    {
        // Mouse menu: 打开面板 hands the foreground back (no activation); keyboard menu: the panel takes it.
        Assert.True(OpenPolicy.RestoresForegroundAfterMenu(1, keyboard: false));
        Assert.False(OpenPolicy.RestoresForegroundAfterMenu(1, keyboard: true));
        Assert.True(OpenPolicy.RestoresForegroundAfterMenu(0, keyboard: true));
        Assert.True(OpenPolicy.RestoresForegroundAfterMenu(2, keyboard: true));
        Assert.False(OpenPolicy.RestoresForegroundAfterMenu(3, keyboard: false));
        Assert.False(OpenPolicy.RestoresForegroundAfterMenu(4, keyboard: false));
    }

    [Fact]
    public void FocusRingOnlyForTheKeyboard()
    {
        Assert.True(OpenPolicy.ShowsFocusRing(OpenSource.TrayKeyboard));
        Assert.True(OpenPolicy.ShowsFocusRing(OpenSource.MenuKeyboard));
        Assert.False(OpenPolicy.ShowsFocusRing(OpenSource.Relaunch));
        Assert.False(OpenPolicy.ShowsFocusRing(OpenSource.Startup));
    }
}

public class DismissPolicyTests
{
    [Fact]
    public void TheOpeningClickLandingOnTheTaskbarDoesNotClose()
    {
        Assert.Null(DismissPolicy.HideReason(DismissSignal.ForegroundChanged, "Shell_TrayWnd", 120, false));
        Assert.Null(DismissPolicy.HideReason(DismissSignal.TaskbarPressed, "Shell_TrayWnd", 399, false));
        Assert.Null(DismissPolicy.HideReason(DismissSignal.ForegroundChanged, "TopLevelWindowForOverflowXamlIsland", 200, false));
    }

    [Fact]
    public void AnotherProgramClosesEvenRightAfterOpening()
    {
        Assert.Equal("foreground-changed fg=Notepad", DismissPolicy.HideReason(DismissSignal.ForegroundChanged, "Notepad", 50, false));
        Assert.Equal("deactivated fg=CabinetWClass", DismissPolicy.HideReason(DismissSignal.Deactivated, "CabinetWClass", 10, false));
    }

    [Fact]
    public void ClickingTheTaskbarLaterCloses()
    {
        Assert.Equal("taskbar-pressed fg=Shell_TrayWnd", DismissPolicy.HideReason(DismissSignal.TaskbarPressed, "Shell_TrayWnd", 2000, false));
        Assert.Equal("taskbar-pressed fg=Shell_SecondaryTrayWnd",
            DismissPolicy.HideReason(DismissSignal.TaskbarPressed, "Shell_SecondaryTrayWnd", 2000, false));
        Assert.Equal("foreground-changed fg=Shell_TrayWnd", DismissPolicy.HideReason(DismissSignal.ForegroundChanged, "Shell_TrayWnd", 900, false));
    }

    [Fact]
    public void PressesOutsideTheTaskbarItselfAreLeftToTheForegroundRule()
    {
        // A press inside the overflow flyout or another Explorer window changes the foreground on its own.
        Assert.Null(DismissPolicy.HideReason(DismissSignal.TaskbarPressed, "TopLevelWindowForOverflowXamlIsland", 3000, false));
        Assert.Null(DismissPolicy.HideReason(DismissSignal.TaskbarPressed, "CabinetWClass", 3000, false));
    }

    [Fact]
    public void APressBackInTheWindowThatKeptTheForegroundCloses()
    {
        // The panel was opened by a start, not by a click in that program: any press there is the user going back.
        Assert.Equal("foreground-app-pressed fg=Notepad", DismissPolicy.HideReason(DismissSignal.ForegroundAppPressed, "Notepad", 50, false));
        Assert.Equal("foreground-app-pressed fg=Notepad", DismissPolicy.HideReason(DismissSignal.ForegroundAppPressed, "Notepad", 600, false));
        Assert.Null(DismissPolicy.HideReason(DismissSignal.ForegroundAppPressed, "Shell_TrayWnd", 100, false));
    }

    [Fact]
    public void OverflowOpensGetALongerGrace()
    {
        Assert.Null(DismissPolicy.HideReason(DismissSignal.ForegroundChanged, "Shell_TrayWnd", 700, false, openedFromOverflow: true));
        Assert.NotNull(DismissPolicy.HideReason(DismissSignal.ForegroundChanged, "Shell_TrayWnd", 700, false));
        Assert.NotNull(DismissPolicy.HideReason(DismissSignal.ForegroundChanged, "Shell_TrayWnd", 1200, false, openedFromOverflow: true));
    }

    [Fact]
    public void OwnPopupsNeverClose()
    {
        Assert.Null(DismissPolicy.HideReason(DismissSignal.Deactivated, "Xaml_WindowedPopupClass", 5000, ownTransient: true));
    }
}

public class TrayRetryTests
{
    [Fact]
    public void BacksOffToFifteenSeconds()
    {
        Assert.Equal(new[] { 500, 1000, 2000, 4000, 8000, 15000, 15000 },
            new[] { 1, 2, 3, 4, 5, 6, 40 }.Select(n => (int)TrayRetry.Delay(n).TotalMilliseconds).ToArray());
    }
}

public class LaunchOptionsTests
{
    [Fact]
    public void PlainStartIsAUserStart()
    {
        var o = LaunchOptions.Parse([]);
        Assert.False(o.Autostart);
        Assert.Equal(DemoScenario.Subscriptions, o.Demo);
        Assert.Null(o.LogDir);
    }

    [Fact]
    public void ReadsTheSignInSwitchAndTestSwitches()
    {
        var o = LaunchOptions.Parse(["--autostart", "--test-log-dir=C:\\logs", "--test-pin", "--test-expand", "--test-scroll-end",
            "--test-demo=single", "--test-rect-file=r.txt", "--test-icon-file=i.txt", "--test-tray-add-failures=2", "--unknown"]);
        Assert.True(o.Autostart);
        Assert.Equal("C:\\logs", o.LogDir);
        Assert.True(o.Pin && o.Expand && o.ScrollEnd);
        Assert.Equal(DemoScenario.Single, o.Demo);
        Assert.Equal(("r.txt", "i.txt", 2), (o.RectFile, o.IconFile, o.TrayAddFailures));
    }
}
