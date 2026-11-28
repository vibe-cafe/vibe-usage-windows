using System.Collections.Frozen;
using System.Globalization;

namespace VibeUsage.Display;

public enum Hero { Cost, Token, ActiveTime }
public enum Metric { Tokens, Cost }
public enum Dimension { Model, Terminal, Tool }

public sealed class UsageFilters
{
    public IReadOnlySet<string> Sources { get; }
    public IReadOnlySet<string> Models { get; }
    public IReadOnlySet<string> Hostnames { get; }

    public UsageFilters(IEnumerable<string>? sources = null, IEnumerable<string>? models = null, IEnumerable<string>? hostnames = null)
    {
        Sources = (sources ?? []).ToFrozenSet(StringComparer.Ordinal);
        Models = (models ?? []).ToFrozenSet(StringComparer.Ordinal);
        Hostnames = (hostnames ?? []).ToFrozenSet(StringComparer.Ordinal);
    }

    internal bool IncludesActivity(UsageBucket b) => (Sources.Count == 0 || Sources.Contains(b.Source))
        && (Hostnames.Count == 0 || Hostnames.Contains(b.Hostname));

    public bool SameActivityConditions(UsageFilters other) =>
        Sources.SetEquals(other.Sources) && Hostnames.SetEquals(other.Hostnames);
}

public sealed record FilterChip(Dimension Dimension, string Title);
public sealed record FilterChips(IReadOnlyList<FilterChip> Chips, string? LockedTool);

public sealed class UsagePanelState
{
    public Provider? SelectedSubscription { get; set; }
    public Hero Hero { get; set; }
    public bool DetailsExpanded { get; set; }
    public bool FilterMenuOpen { get; set; }
    public int Presentation { get; private set; }
    public bool UsesCny { get; private set; }
    public bool UsesChineseTokens { get; private set; }
    public UsageRange Range { get; set; } = UsageRange.Today;
    public UsageFilters Filters { get; set; } = new();
    private readonly Dictionary<Dimension, Metric> metrics = [];
    public Metric this[Dimension card]
    {
        get => metrics.GetValueOrDefault(card);
        set => metrics[card] = value;
    }

    public void PanelDidOpen()
    {
        SelectedSubscription = null;
        Hero = Hero.Cost;
        DetailsExpanded = false;
        FilterMenuOpen = false;
        Presentation++;
    }

    public void ToggleHeroUnit()
    {
        if (Hero == Hero.Cost) UsesCny = !UsesCny;
        if (Hero == Hero.Token) UsesChineseTokens = !UsesChineseTokens;
    }

    public void ClearFilters()
    {
        Filters = new();
        FilterMenuOpen = false;
    }

    private Provider? LockedSubscription(IReadOnlyList<Provider> enabled) =>
        enabled.Count > 1 && SelectedSubscription is { } p && enabled.Contains(p) ? p : null;

    public UsageFilters EffectiveFilters(IReadOnlyList<Provider> enabled) =>
        LockedSubscription(enabled) is { } p ? new([p.UsageSource()], Filters.Models, Filters.Hostnames) : Filters;

    public FilterChips Chips(IReadOnlyList<Provider> enabled, UsageNames? names = null)
    {
        string ToolName(string id) => names?.ToolName(id) ?? id;
        var locked = LockedSubscription(enabled);
        string? lockedName = null;
        if (locked is { } p)
        {
            var name = ToolName(p.UsageSource());
            lockedName = name == p.UsageSource() ? p.DisplayName() : name;
        }
        var chips = new List<FilterChip>();
        void Add(Dimension d, IEnumerable<string> names)
        {
            var sorted = names.Order(Comparer<string>.Create((a, b) => CultureInfo.CurrentCulture.CompareInfo.Compare(
                a, b, CompareOptions.NumericOrdering | CompareOptions.IgnoreCase))).ToArray();
            if (sorted.Length > 0) chips.Add(new(d, sorted.Length == 1 ? sorted[0] : $"{sorted[0]} +{sorted.Length - 1}"));
        }
        Add(Dimension.Terminal, Filters.Hostnames);
        if (locked == null) Add(Dimension.Tool, Filters.Sources.Select(ToolName));
        Add(Dimension.Model, Filters.Models);
        return new(chips, lockedName);
    }
}
