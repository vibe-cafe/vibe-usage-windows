using System;
using System.Linq;

namespace VibeUsage.App.Panel;

/// <summary>
/// What 「同步用量」 in the footer says (Mac <c>ModernSyncLabel</c>, design
/// 136c, 136e–136g). Pure, so the wording is unit-tested.
/// </summary>
public abstract record SyncLabel
{
    public sealed record Syncing : SyncLabel;

    /// <summary>「同步出错」 + the first line of the reason + 「重试」; <paramref name="Detail"/> is the full message (tooltip).</summary>
    public sealed record Failed(string Reason, string Detail) : SyncLabel;

    /// <summary>「26 分钟前同步」: <paramref name="Number"/> is set in the number face.</summary>
    public sealed record Synced(string? Number, string Text) : SyncLabel;

    public sealed record Never : SyncLabel;

    private const string FailurePrefix = "同步失败: ";

    /// <param name="status">The backend's <c>sync-state</c> status: idle, syncing, success or error.</param>
    public static SyncLabel Make(string status, string? message, DateTimeOffset? lastSuccess, DateTimeOffset now)
    {
        switch (status)
        {
            case "syncing":
                return new Syncing();
            case "error":
                var detail = message ?? "";
                var reason = detail.StartsWith(FailurePrefix, StringComparison.Ordinal) ? detail[FailurePrefix.Length..] : detail;
                reason = reason.Split('\n', '\r').FirstOrDefault() ?? "";
                return new Failed(reason.Trim(), detail);
            default:
                if (lastSuccess is not DateTimeOffset last) return new Never();
                var relative = Relative(last, now);
                var digits = new string(relative.TakeWhile(char.IsAsciiDigit).ToArray());
                return digits.Length == 0
                    ? new Synced(null, relative + "同步")
                    : new Synced(digits, relative[digits.Length..] + "同步");
        }
    }

    /// <summary>Mac <c>Formatters.formatRelativeTime</c>.</summary>
    public static string Relative(DateTimeOffset date, DateTimeOffset now)
    {
        var seconds = (now - date).TotalSeconds;
        if (seconds < 60) return "刚刚";
        if (seconds < 3600) return $"{(int)(seconds / 60)} 分钟前";
        if (seconds < 86400) return $"{(int)(seconds / 3600)} 小时前";
        return $"{(int)(seconds / 86400)} 天前";
    }
}
