using static VibeUsage.Display.Tests.UsageDisplayRulesTests;

namespace VibeUsage.Display.Tests;

public class UsageBucketTests
{
    private static UsageBucket[] BothFormats()
    {
        var legacy = B("2026-11-26T02:00Z", 120) with { OutputTokens = 20, ReasoningOutputTokens = 5, CachedInputTokens = 40 };
        var separateWrites = legacy with { BucketStart = Now, InputTokens = 100, CacheCreation5mTokens = 7, CacheCreation1hTokens = 13 };
        return [legacy, separateWrites];
    }

    [Fact]
    public void panelAndMenuBarCountCacheWritesOnceAcrossBothBucketFormats()
    {
        var data = BothFormats();
        var range = new UsageRange(Period.SevenDays);
        foreach (var b in data) Assert.Equal(185, Snap([b], range).Total.TotalTokens);
        Assert.Equal(370, Snap(data, range).Total.TotalTokens);
        Assert.Equal(0, new UsageDisplaySnapshot(data, range, Now, Zone, new(["cursor"])).Total.TotalTokens);
    }

    [Fact]
    public void trendCountsCacheWritesAsInputAndOnlyReadsAsCached()
    {
        var snap = Snap(BothFormats(), new(Period.SevenDays));
        Assert.Equal([new UsageAmount(120, 25, 40, 0), new UsageAmount(120, 25, 40, 0)], snap.Series.Where(a => a.TotalTokens > 0));
    }
}
