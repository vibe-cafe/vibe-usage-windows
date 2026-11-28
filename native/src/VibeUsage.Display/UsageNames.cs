namespace VibeUsage.Display;

public sealed record UsageFamily(string Key, string Label);
public enum FamilySelection { Off, Mixed, On }
public sealed record ModelFilterGroup(string Key, string Label, IReadOnlyList<string> Models)
{
    public FamilySelection Selection(UsageFilters filters)
    {
        var count = Models.Count(filters.Models.Contains);
        return count == 0 ? FamilySelection.Off : count == Models.Count ? FamilySelection.On : FamilySelection.Mixed;
    }

    public UsageFilters Toggle(UsageFilters filters)
    {
        var next = Selection(filters) == FamilySelection.On ? filters.Models.Except(Models) : filters.Models.Union(Models);
        return new(filters.Sources, next, filters.Hostnames);
    }
}

public sealed class UsageNames
{
    public IReadOnlyDictionary<string, string> Sources { get; init; } = new Dictionary<string, string>();
    public IReadOnlyDictionary<string, string> Models { get; init; } = new Dictionary<string, string>();
    public IReadOnlyDictionary<string, string> ModelFamilies { get; init; } = new Dictionary<string, string>();
    public IReadOnlyList<UsageFamily> Families { get; init; } = [];
    public string ToolName(string source) => Sources.GetValueOrDefault(source, source);
    public string ModelName(string model) => Models.GetValueOrDefault(model, model);

    public IReadOnlyList<ModelFilterGroup> FilterGroups(IEnumerable<UsageBucket> buckets)
    {
        var byName = new SortedDictionary<string, string?>(StringComparer.Ordinal);
        foreach (var b in buckets)
        {
            var name = ModelName(b.Model);
            if (byName.GetValueOrDefault(name) == null) byName[name] = ModelFamilies.GetValueOrDefault(b.Model);
        }
        var members = byName.ToLookup(p => p.Value, p => p.Key);
        var groups = Families.Where(f => members.Contains(f.Key))
            .Select(f => new ModelFilterGroup(f.Key, f.Label, members[f.Key].ToArray())).ToList();
        if (members.Contains(null)) groups.Add(new("other", "其他", members[null].ToArray()));
        return groups;
    }
}
