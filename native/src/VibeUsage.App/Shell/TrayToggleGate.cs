namespace VibeUsage.App.Shell;

/// <summary>
/// Decides what a tray click means, as a pure state machine (unit-tested).
///
/// The race it handles: with the panel open, pressing the mouse on the tray
/// icon first activates the taskbar, so the panel loses activation and closes;
/// only then, on mouse-up, does the click (NIN_SELECT) arrive. Taken at face
/// value that click would open the panel again ("closes and pops right back").
///
/// Two signals mark such a click as the one that closed the panel:
/// 1. When the panel auto-closed, the cursor was on the tray icon, or the
///    icon reported a button-down shortly after the auto-close. The next
///    click within <see cref="SwallowExpiryMs"/> is swallowed. This covers a
///    slow press-and-hold, which a pure timer would miss.
/// 2. Fallback: any click within <see cref="AutoCloseGraceMs"/> of an
///    auto-close (the Tauri prototype's 350 ms rule).
/// </summary>
public sealed class TrayToggleGate
{
    public const long AutoCloseGraceMs = 300;
    public const long ButtonDownWindowMs = 600;
    public const long SwallowExpiryMs = 1500;
    public const long KeyRepeatMs = 250;

    public enum Decision { Open, Close, Swallow, IgnoreRepeat }

    private long _lastAutoClose = long.MinValue / 2;
    private long _swallowUntil = long.MinValue / 2;
    private long _lastKeySelect = long.MinValue / 2;

    /// <summary>The panel closed by itself (lost activation, foreground moved).</summary>
    public void OnAutoClose(long now, bool cursorOnIcon)
    {
        _lastAutoClose = now;
        if (cursorOnIcon) _swallowUntil = now + SwallowExpiryMs;
    }

    /// <summary>The icon reported a left button-down.</summary>
    public void OnButtonDown(long now, bool panelVisible)
    {
        if (!panelVisible && now - _lastAutoClose <= ButtonDownWindowMs) _swallowUntil = now + SwallowExpiryMs;
    }

    /// <summary>Right-click menu, explicit open or close: forget pending swallows.</summary>
    public void Reset()
    {
        _swallowUntil = long.MinValue / 2;
        _lastAutoClose = long.MinValue / 2;
    }

    public Decision OnSelect(long now, bool panelVisible, bool keyboard)
    {
        if (keyboard)
        {
            // Enter on a focused tray icon can arrive as two NIN_KEYSELECTs.
            if (now - _lastKeySelect < KeyRepeatMs) return Decision.IgnoreRepeat;
            _lastKeySelect = now;
        }

        if (panelVisible)
        {
            Reset();
            return Decision.Close;
        }

        if (now < _swallowUntil)
        {
            Reset();
            return Decision.Swallow;
        }

        if (now - _lastAutoClose < AutoCloseGraceMs)
        {
            Reset();
            return Decision.Swallow;
        }

        return Decision.Open;
    }
}
