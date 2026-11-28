using System;
using System.Linq;
using System.Runtime.InteropServices;
using System.Threading;
using Microsoft.UI.Xaml;
using VibeUsage.PanelPoc.Data;
using VibeUsage.PanelPoc.Shell;

namespace VibeUsage.PanelPoc;

/// <summary>
/// Tray app: no main window. A hidden tray-host window owns the icon, the
/// panel is created hidden and toggled from the icon, settings is a normal
/// window. One instance per user; launching the exe again opens the panel of
/// the running one.
/// </summary>
public partial class App : Application
{
    private const string InstanceMutexName = @"Local\VibeUsagePanelPoc.SingleInstance";

    private Mutex? _instanceMutex;
    private PocOptions _options = PocOptions.Parse(Array.Empty<string>());
    private TrayIcon? _tray;
    private PanelWindow? _panel;
    private FlyoutController? _flyout;
    private SettingsWindow? _settings;

    public App()
    {
        try
        {
            InitializeComponent();
        }
        catch (Exception ex)
        {
            PocLog.Write("App resources failed to load: " + ex);
            throw;
        }
        UnhandledException += (_, e) => PocLog.Write("unhandled: " + e.Exception);
    }

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        try
        {
            Launch();
        }
        catch (Exception ex)
        {
            PocLog.Write("launch failed: " + ex);
            throw;
        }
    }

    private void Launch()
    {
        var argv = Environment.GetCommandLineArgs().Skip(1).ToArray();
        _options = PocOptions.Parse(argv);
        PocLog.UseDirectory(_options.LogDir);

        _instanceMutex = new Mutex(true, InstanceMutexName, out bool first);
        if (!first)
        {
            HandOffToRunningInstance();
            Exit();
            return;
        }

        PocLog.Write($"startup os={Environment.OSVersion.Version} arch={RuntimeInformation.ProcessArchitecture} " +
                     $"args=[{string.Join(' ', argv)}] acrylic={FlyoutBackdrop.IsSupported}");

        _panel = new PanelWindow(_options);
        _tray = new TrayIcon();
        _tray.SetTooltip(FakeData.TrayTooltip);
        _flyout = new FlyoutController(_panel, _tray, _options)
        {
            SettingsWindowHandle = () => _settings?.Hwnd ?? IntPtr.Zero,
        };

        _tray.ContextMenuRequested += OnTrayContextMenu;
        _tray.OpenRequested += () => _flyout.Show(null, ShowSource.External);
        _panel.SettingsRequested += () =>
        {
            _flyout.Hide("open-settings", auto: false);
            OpenSettings();
        };

        // Launched by the user: open the panel so they see where the app went.
        // With --autostart (the sign-in entry of a real build) stay in the tray.
        bool openNow = _options.OpenAtStart || !argv.Contains("--autostart");
        _flyout.WarmUp(() =>
        {
            if (openNow) _flyout.Show(null, ShowSource.Startup);
        });
    }

    private static void HandOffToRunningInstance()
    {
        var host = Win32.FindWindowW(TrayIcon.HostClassName, null);
        if (host == IntPtr.Zero) return;
        // This process was just started by the user, so it may hand its
        // foreground right to the running instance.
        Win32.AllowSetForegroundWindow(Win32.ProcessOf(host));
        Win32.PostMessageW(host, TrayIcon.WM_OPEN_REQUEST, IntPtr.Zero, IntPtr.Zero);
    }

    private void OnTrayContextMenu(Win32.POINT anchor)
    {
        if (_tray is null || _flyout is null || _panel is null) return;
        _flyout.PrepareForMenu();
        uint cmd = _tray.ShowMenu(anchor, _flyout.EdgeAt(anchor), new (uint, string?)[]
        {
            (1, "打开面板"),
            (0, null),
            (2, "立即同步"),
            (3, "设置"),
            (0, null),
            (4, "退出"),
        }, defaultId: 1);

        switch (cmd)
        {
            case 1:
                _flyout.Show(anchor, ShowSource.Menu);
                break;
            case 2:
                _panel.StartFakeSync();
                break;
            case 3:
                OpenSettings();
                break;
            case 4:
                Quit();
                break;
        }
    }

    private void OpenSettings()
    {
        if (_settings is null)
        {
            _settings = new SettingsWindow();
            _settings.Closed += (_, _) => _settings = null;
        }
        _settings.Activate();
        Win32.SetForegroundWindow(_settings.Hwnd);
    }

    private void Quit()
    {
        PocLog.Write("quit");
        _flyout?.Dispose();
        _tray?.Dispose();
        if (_panel != null) _panel.AllowClose = true;
        _settings?.Close();
        _panel?.Close();
        _instanceMutex?.Dispose();
        Exit();
    }
}
