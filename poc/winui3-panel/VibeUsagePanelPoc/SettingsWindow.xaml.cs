using System;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using VibeUsage.PanelPoc.Shell;
using Windows.Graphics;
using WinRT.Interop;

namespace VibeUsage.PanelPoc;

/// <summary>Settings placeholder: a regular Windows 11 window (Mica, content under the title bar).</summary>
public sealed partial class SettingsWindow : Window
{
    public IntPtr Hwnd { get; }

    public SettingsWindow()
    {
        InitializeComponent();
        Hwnd = WindowNative.GetWindowHandle(this);
        ExtendsContentIntoTitleBar = true;
        SetTitleBar(AppTitleBar);
        AppWindow.SetIcon(System.IO.Path.Combine(AppContext.BaseDirectory, "Assets", "AppIcon.ico"));

        double scale = Win32.GetDpiForWindow(Hwnd) / 96.0;
        if (scale <= 0) scale = 1;
        var size = new SizeInt32((int)(760 * scale), (int)(600 * scale));
        var area = DisplayArea.GetFromWindowId(AppWindow.Id, DisplayAreaFallback.Primary).WorkArea;
        AppWindow.MoveAndResize(new RectInt32(
            area.X + Math.Max(0, (area.Width - size.Width) / 2),
            area.Y + Math.Max(0, (area.Height - size.Height) / 2),
            Math.Min(size.Width, area.Width),
            Math.Min(size.Height, area.Height)));
    }
}
