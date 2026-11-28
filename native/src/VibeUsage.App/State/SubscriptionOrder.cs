using System.Collections.Generic;
using System.Linq;
using VibeUsage.Backend;

namespace VibeUsage.App.State;

/// <summary>Where a dragged round button goes in the order Settings stores (Mac <c>AppState.moveQuotaProduct</c>).</summary>
public static class SubscriptionOrder
{
    /// <summary>
    /// <paramref name="provider"/> moved before <paramref name="before"/>, or
    /// right after the last enabled product when null ("past the last
    /// button"). Enabled products missing from the stored order are appended
    /// first. Null when nothing changes or the product is not enabled.
    /// </summary>
    public static Provider[]? Move(IReadOnlyList<Provider> stored, IReadOnlyList<Provider> enabled, Provider provider, Provider? before)
    {
        if (provider == before || !enabled.Contains(provider)) return null;
        var order = stored.ToList();
        foreach (var p in enabled) if (!order.Contains(p)) order.Add(p);
        var old = order.ToArray();
        order.Remove(provider);
        int to = before is { } target && order.IndexOf(target) is int i and >= 0
            ? i
            : order.FindLastIndex(enabled.Contains) + 1;
        order.Insert(to, provider);
        return order.SequenceEqual(old) ? null : order.ToArray();
    }
}
