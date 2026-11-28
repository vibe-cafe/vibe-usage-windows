using static VibeUsage.Display.Tests.UsageDisplayRulesTests;

namespace VibeUsage.Display.Tests;

public class UsageNamesTests
{
    private static readonly UsageBucket[] Buckets = [
        B("2026-11-27T01:00Z",11,source:"kimi-code",model:"k3"),
        B("2026-11-27T02:00Z",22,source:"opencode",model:"kimi-code/k3-256k")];
    private static readonly UsageNames Names = new()
    {
        Sources = new Dictionary<string, string> { ["kimi-code"] = "Kimi Code", ["opencode"] = "OpenCode" },
        Models = new Dictionary<string, string> { ["k3"] = "Kimi K3", ["kimi-code/k3-256k"] = "Kimi K3" },
        ModelFamilies = new Dictionary<string, string> { ["k3"] = "kimi", ["kimi-code/k3-256k"] = "kimi" },
        Families = [new("kimi", "Kimi")]
    };

    [Fact]
    public void theServerNamesReachTheApp()
    {
        Assert.Equal("Kimi Code", Names.ToolName("kimi-code"));
        Assert.Equal("OpenCode", Names.ToolName("opencode"));
        Assert.Equal("Kimi K3", Names.ModelName("k3"));
        Assert.Equal("acme", Names.ModelName("acme"));
        Assert.Equal("new-cli", Names.ToolName("new-cli"));
    }

    [Fact]
    public void aResponseWithoutNamesDecodesAndShowsRawIds()
    {
        var names = new UsageNames();
        var group = Assert.Single(names.FilterGroups(Buckets));
        Assert.Equal(("other", "其他"), (group.Key, group.Label));
        Assert.Equal(["k3", "kimi-code/k3-256k"], group.Models);
        Assert.Equal("kimi-code", names.ToolName("kimi-code"));
    }

    [Fact]
    public void selectingAMergedNameMatchesEveryRawIdBehindIt()
    {
        UsageDisplaySnapshot Filter(string model) => new(Buckets, UsageRange.Today, Now, Zone, new(models: [model]), Names);
        Assert.Equal(33, Filter("Kimi K3").Total.TotalTokens);
        Assert.Empty(Filter("Kimi K2").Series);
    }

    [Fact]
    public void modelFilterGroupsFollowTheServerFamilies()
    {
        var group = Assert.Single(Names.FilterGroups(Buckets));
        Assert.Equal(("kimi", "Kimi", "Kimi K3"), (group.Key, group.Label, Assert.Single(group.Models)));
    }

    [Fact]
    public void unplacedNamesFallIntoOther()
    {
        var groups = Names.FilterGroups([.. Buckets, B("2026-11-27T02:00Z", model: "unplaced")]);
        Assert.Equal(["kimi", "other"], groups.Select(g => g.Key));
        Assert.Equal(["unplaced"], groups[^1].Models);
    }

    [Fact]
    public void mixedFamilySelectsAllThenRemovesOnlyItsMembers()
    {
        var group = new ModelFilterGroup("family", "Family", ["A", "B"]);
        var original = new UsageFilters(["cursor"], ["A", "C"], ["pc"]);
        Assert.Equal(FamilySelection.Mixed, group.Selection(original));
        var all = group.Toggle(original);
        Assert.Equal(FamilySelection.On, group.Selection(all));
        Assert.True(all.Models.SetEquals(["A", "B", "C"]));
        var cleared = group.Toggle(all);
        Assert.Equal(FamilySelection.Off, group.Selection(cleared));
        Assert.Equal("C", Assert.Single(cleared.Models));
        Assert.True(cleared.SameActivityConditions(original));
    }
}
