namespace VibeUsage.Display;

public enum Period { Today, RollingDay, SevenDays, ThirtyDays, NinetyDays, Custom }
public readonly record struct Slot(DateTimeOffset Start, DateTimeOffset End);

public sealed record UsageRange
{
    public Period Period { get; }
    public DateOnly? From { get; }
    public DateOnly? To { get; }
    public static UsageRange Today { get; } = new(Period.Today);
    public static IReadOnlyList<UsageRange> QuickPeriods { get; } = [Today, new(Period.RollingDay), new(Period.SevenDays), new(Period.ThirtyDays), new(Period.NinetyDays)];

    public UsageRange(Period period)
    {
        if (period == Period.Custom) throw new ArgumentException("Supply both local dates for a custom range.", nameof(period));
        Period = period;
    }

    public UsageRange(DateOnly from, DateOnly to)
    {
        Period = Period.Custom;
        (From, To) = from <= to ? (from, to) : (to, from);
    }

    public string Label => Period switch
    {
        Period.Today => "今天",
        Period.RollingDay => "24H",
        Period.SevenDays => "7D",
        Period.ThirtyDays => "30D",
        Period.NinetyDays => "90D",
        _ => "自定义日期"
    };
    public string ChipLabel => From is { } from && To is { } to
        ? from == to ? $"{from.Month}/{from.Day}" : $"{from.Month}/{from.Day}–{to.Month}/{to.Day}" : Label;
    public bool Hourly => Period is Period.Today or Period.RollingDay || Period == Period.Custom && From == To;

    internal static DateOnly LocalDate(DateTimeOffset instant, TimeZoneInfo zone) =>
        DateOnly.FromDateTime(TimeZoneInfo.ConvertTime(instant, zone).DateTime);

    internal static DateTimeOffset Midnight(DateOnly date, TimeZoneInfo zone)
    {
        var wall = date.ToDateTime(TimeOnly.MinValue);
        // A midnight transition can skip or repeat the first hour of the day.
        while (zone.IsInvalidTime(wall)) wall = wall.AddMinutes(1);
        var offset = zone.IsAmbiguousTime(wall) ? zone.GetAmbiguousTimeOffsets(wall).Max() : zone.GetUtcOffset(wall);
        return new DateTimeOffset(wall, offset);
    }

    public Slot Bounds(DateTimeOffset now, TimeZoneInfo zone)
    {
        var today = LocalDate(now, zone);
        var last = To ?? today;
        var count = Period switch { Period.SevenDays => 7, Period.ThirtyDays => 30, Period.NinetyDays => 90, _ => 1 };
        var start = Period == Period.RollingDay ? now.AddHours(-24) : Midnight(From ?? today.AddDays(1 - count), zone);
        return new(start, Midnight(last.AddDays(1), zone));
    }

    public DateTimeOffset? InclusiveCustomEnd(TimeZoneInfo zone) =>
        To is { } to ? Midnight(to.AddDays(1), zone).AddMilliseconds(-1) : null;
}

public sealed class UsagePlotGrid
{
    public IReadOnlyList<Slot> Slots { get; }
    public UsageRange Range { get; }
    public TimeZoneInfo Zone { get; }
    public DateTimeOffset Now { get; }
    private readonly DateTimeOffset firstServerHour;
    private readonly DateTimeOffset endExclusive;

    public UsagePlotGrid(UsageRange range, DateTimeOffset now, TimeZoneInfo zone)
    {
        Range = range;
        Zone = zone;
        Now = now;
        var bounds = range.Bounds(now, zone);
        endExclusive = bounds.End;
        var day = UsageRange.LocalDate(bounds.Start, zone);
        var endDay = UsageRange.LocalDate(endExclusive, zone);
        var days = endDay.DayNumber - day.DayNumber;
        var weekly = !range.Hourly && days > 60;
        var step = weekly ? (days - 1) % 7 + 1 : 1;
        firstServerHour = FloorHour(bounds.Start);
        var cursor = range.Hourly ? firstServerHour : bounds.Start;
        var slots = new List<Slot>();
        while (cursor <= now && bounds.Start <= now && cursor < endExclusive)
        {
            var next = range.Hourly ? cursor.AddHours(1) : UsageRange.Midnight(day = day.AddDays(step), zone);
            var end = next < endExclusive ? next : endExclusive;
            slots.Add(new(cursor < bounds.Start ? bounds.Start : cursor, end < now ? end : now));
            cursor = next;
            step = weekly ? 7 : 1;
        }
        Slots = slots;
    }

    private UsagePlotGrid(UsagePlotGrid grid, int skip)
    {
        Range = grid.Range;
        Zone = grid.Zone;
        Now = grid.Now;
        firstServerHour = grid.firstServerHour;
        endExclusive = grid.endExclusive;
        Slots = grid.Slots.Skip(skip).ToArray();
    }

    internal UsagePlotGrid ForUsage(int? first) => first == null ? new(this, Slots.Count)
        : Range.Period == Period.Today && first > 0 ? new(this, first.Value) : this;

    private static DateTimeOffset FloorHour(DateTimeOffset instant)
    {
        long ticks = instant.UtcTicks;
        return new(ticks - ticks % TimeSpan.TicksPerHour, TimeSpan.Zero);
    }

    internal int Index(DateTimeOffset time)
    {
        if (Slots.Count == 0 || time > Now || time >= endExclusive) return -1;
        if (time < Slots[0].Start)
            return time == firstServerHour && Slots[0].Start - time < TimeSpan.FromHours(1) ? 0 : -1;
        int low = 0, high = Slots.Count;
        while (low + 1 < high)
        {
            var mid = (low + high) / 2;
            if (Slots[mid].Start <= time) low = mid;
            else high = mid;
        }
        return low;
    }
}
