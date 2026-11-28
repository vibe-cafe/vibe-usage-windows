using System;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using VibeUsage.App.Panel;
using VibeUsage.App.Shell;
using Windows.Graphics;
using WinRT.Interop;

namespace VibeUsage.App.Settings;

/// <summary>
/// The settings window's frame: a normal Windows window (taskbar, Alt+Tab,
/// the system close button), dark title bar, one instance, closing only
/// hides it. Its five pages come with the settings ticket; until then it is
/// empty, so the panel's 「设置」, the gear and the tray menu have their
/// target and the hand-over (panel closes, settings comes to the front) can
/// be tested.
/// </summary>
internal sealed class SettingsWindow : Window
{
    private bool sized;

    public IntPtr Hwnd { get; }

    public bool AllowClose { get; set; }

    public SettingsWindow()
    {
        Title = "Vibe Usage 设置";
        Content = new Grid { RequestedTheme = ElementTheme.Dark, Background = new SolidColorBrush(Theme.Panel) };
        Hwnd = WindowNative.GetWindowHandle(this);
        var presenter = OverlappedPresenter.Create();
        presenter.IsMaximizable = false;
        AppWindow.SetPresenter(presenter);
        AppWindow.SetIcon(System.IO.Path.Combine(AppContext.BaseDirectory, "Assets", "AppIcon.ico"));
        Win32.SetDwmInt(Hwnd, Win32.DWMWA_USE_IMMERSIVE_DARK_MODE, 1);
        AppWindow.Closing += (_, e) =>
        {
            if (AllowClose) return;
            e.Cancel = true;
            AppWindow.Hide();
            ShellLog.Write("settings hidden");
        };
    }

    /// <summary>Shows it in front, 700 × 460 DIP centred on its screen the first time (Mac default size).</summary>
    public void ShowInFront()
    {
        if (!sized)
        {
            sized = true;
            double scale = Win32.GetDpiForWindow(Hwnd) / 96.0;
            var size = new SizeInt32((int)Math.Round(700 * scale), (int)Math.Round(460 * scale));
            var work = DisplayArea.GetFromWindowId(AppWindow.Id, DisplayAreaFallback.Primary).WorkArea;
            AppWindow.MoveAndResize(new RectInt32(work.X + (work.Width - size.Width) / 2, work.Y + (work.Height - size.Height) / 2,
                size.Width, size.Height));
        }
        Activate();
        bool fg = Win32.SetForegroundWindow(Hwnd);
        ShellLog.Write($"settings shown foreground={fg}");
    }
}
