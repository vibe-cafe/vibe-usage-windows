using Microsoft.UI.Composition;
using Microsoft.UI.Composition.SystemBackdrops;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Media;

namespace VibeUsage.PanelPoc.Shell;

/// <summary>
/// Desktop Acrylic, the material Windows 11 uses for transient surfaces
/// (quick settings, the calendar, menus). Mica is meant for long-lived app
/// windows, so it is used for the settings window instead.
///
/// The built-in DesktopAcrylicBackdrop turns solid whenever the window is
/// inactive. A flyout is only ever on screen while it is in use, and if
/// Windows declines to activate it (see FlyoutController) it should still look
/// like a flyout, so this backdrop always reports "input active". Theme
/// follows the panel's ActualTheme; Windows itself falls back to a solid
/// colour when transparency effects are off or on battery saver.
/// </summary>
internal sealed class FlyoutBackdrop : SystemBackdrop
{
    private DesktopAcrylicController? _controller;
    private SystemBackdropConfiguration? _config;
    private SystemBackdropTheme _theme = SystemBackdropTheme.Default;

    public static bool IsSupported => DesktopAcrylicController.IsSupported();

    public void SetTheme(ElementTheme theme)
    {
        _theme = theme switch
        {
            ElementTheme.Dark => SystemBackdropTheme.Dark,
            ElementTheme.Light => SystemBackdropTheme.Light,
            _ => SystemBackdropTheme.Default,
        };
        if (_config != null) _config.Theme = _theme;
    }

    protected override void OnTargetConnected(ICompositionSupportsSystemBackdrop connectedTarget, XamlRoot xamlRoot)
    {
        base.OnTargetConnected(connectedTarget, xamlRoot);
        _config = new SystemBackdropConfiguration { IsInputActive = true, Theme = _theme };
        _controller = new DesktopAcrylicController();
        _controller.AddSystemBackdropTarget(connectedTarget);
        _controller.SetSystemBackdropConfiguration(_config);
        PocLog.Write("backdrop: desktop acrylic connected");
    }

    protected override void OnTargetDisconnected(ICompositionSupportsSystemBackdrop disconnectedTarget)
    {
        base.OnTargetDisconnected(disconnectedTarget);
        if (_controller != null)
        {
            _controller.RemoveSystemBackdropTarget(disconnectedTarget);
            _controller.Dispose();
            _controller = null;
        }
    }
}
