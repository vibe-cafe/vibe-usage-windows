namespace VibeUsage.Display.Tests;

public class ModernPanelTests
{
    [Fact]
    public void refreshScopeFollowsWhatIsOnScreen()
    {
        static QuotaPanelDisplay Panel(Provider[] enabled, Provider? selected = null) =>
            new([], enabled, selected, DateTimeOffset.UnixEpoch);
        Assert.Empty(Panel([]).RefreshTargets);
        Assert.Equal([Provider.Cursor], Panel([Provider.Cursor]).RefreshTargets);
        Provider[] both = [Provider.Codex, Provider.ClaudeCode];
        Assert.Equal(both, Panel(both).RefreshTargets);
        Assert.Equal([Provider.ClaudeCode], Panel(both, Provider.ClaudeCode).RefreshTargets);
        Assert.Equal(both, Panel(both, Provider.Grok).RefreshTargets);
    }

    [Fact]
    public void refreshSpinnerFollowsTheScope()
    {
        QuotaPanelDisplay Panel(Provider? selected) => new([], [Provider.Codex, Provider.KimiCode], selected,
            DateTimeOffset.UnixEpoch, new HashSet<Provider> { Provider.KimiCode });
        Assert.True(Panel(null).IsRefreshing);
        Assert.True(Panel(Provider.KimiCode).IsRefreshing);
        Assert.False(Panel(Provider.Codex).IsRefreshing);
    }

    [Fact]
    public void realOpenReturnsToOverviewCostAndCollapsed()
    {
        var state = new UsagePanelState { SelectedSubscription = Provider.Cursor, Hero = Hero.ActiveTime, DetailsExpanded = true };
        state.PanelDidOpen();
        Assert.Null(state.SelectedSubscription);
        Assert.Equal(Hero.Cost, state.Hero);
        Assert.False(state.DetailsExpanded);
        Assert.Equal(1, state.Presentation);
    }
}
