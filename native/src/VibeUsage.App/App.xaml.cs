using System;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Threading;
using System.Threading.Tasks;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using VibeUsage.App.Panel;
using VibeUsage.App.Settings;
using VibeUsage.App.Shell;
using VibeUsage.App.State;

namespace VibeUsage.App;

/// <summary>
/// Tray app: no main window. A hidden tray-host window owns the icon; the
/// panel is created hidden, warmed up, and toggled from the icon; settings is
/// a normal window. A start by the user opens the panel (activated);
/// --autostart stays in the tray. Quitting hides the windows, removes the
/// icon, closes the backend (it ends its child processes) and exits.
/// </summary>
public partial class ShellApp : Application
{
    private readonly LaunchOptions options;
    private TrayIcon? tray;
    private AppModel? model;
    private PanelWindow? panel;
    private FlyoutController? flyout;
    private SettingsWindow? settings;
    private DispatcherQueueTimer? iconFileTimer;
    private bool quitting;

    public ShellApp(LaunchOptions options)
    {
        this.options = options;
        InitializeComponent();
        // Windows are only hidden, never closed, until Quit; nothing else ends the app.
        DispatcherShutdownMode = DispatcherShutdownMode.OnExplicitShutdown;
        UnhandledException += (_, e) => ShellLog.Write("unhandled: " + e.Exception);
    }

    protected override async void OnLaunched(LaunchActivatedEventArgs args)
    {
        try
        {
            await LaunchAsync();
        }
        catch (Exception ex)
        {
            ShellLog.Write("launch failed: " + ex);
            throw;
        }
    }

    private async Task LaunchAsync()
    {
        ShellLog.Write($"startup version={typeof(ShellApp).Assembly.GetName().Version} os={Environment.OSVersion.Version} " +
                       $"arch={RuntimeInformation.ProcessArchitecture} autostart={options.Autostart}");
        var dispatcher = DispatcherQueue.GetForCurrentThread();

        // The icon first: at sign-in it is the only thing the user sees.
        tray = new TrayIcon(options.TrayAddFailures);
        tray.SessionEnding += EndSession;

        var context = SynchronizationContext.Current ?? new DispatcherQueueSynchronizationContext(dispatcher);
        model = new AppModel(DemoBackend.Create(options.Demo, context));
        model.ExitRequested += () => dispatcher.TryEnqueue(Quit);
        try
        {
            await model.StartAsync();
        }
        catch (Exception ex)
        {
            ShellLog.Write($"backend start failed: {ex.GetType().Name}");
        }

        panel = new PanelWindow(model, options);
        flyout = new FlyoutController(panel, tray, options) { SettingsWindowHandle = () => settings?.Hwnd ?? IntPtr.Zero };
        flyout.Opening += _ => model.PanelOpening();
        flyout.Hidden += panel.OnHidden;
        tray.ContextMenuRequested += OnTrayContextMenu;
        tray.OpenRequested += activate =>
        {
            ShellLog.Write($"open request from a second start activate={activate}");
            flyout.Show(null, OpenSource.Relaunch);
        };
        panel.QuitRequested += Quit;
        panel.SettingsRequested += () =>
        {
            flyout.Hide("open-settings", auto: false);
            OpenSettings();
        };
        if (options.IconFile is string file) StartIconFile(file, dispatcher);

        // Started by the user: open the panel so they see where the app went.
        // --autostart (the sign-in entry) stays in the tray.
        flyout.WarmUp(openAfterwards: !options.Autostart);
    }

    private void OnTrayContextMenu(Win32.POINT anchor)
    {
        if (tray is null || flyout is null || model is null) return;
        flyout.PrepareForMenu();
        uint cmd = tray.ShowMenu(anchor, flyout.EdgeAt(anchor), new (uint, string?)[]
        {
            (1, "打开面板"),
            (0, null),
            (2, "立即同步"),
            (3, "设置"),
            (0, null),
            (4, "退出"),
        }, defaultId: 1);

        // Dismissed, 打开面板 and 立即同步 hand the foreground back to where it
        // was; 设置 and 退出 don't need it.
        if (cmd is 0 or 1 or 2) tray.RestoreForegroundAfterMenu();
        switch (cmd)
        {
            case 1:
                flyout.Show(anchor, OpenSource.Menu);
                break;
            case 2:
                model.TriggerSync();
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
        settings ??= new SettingsWindow();
        settings.ShowInFront();
        model?.SettingsShown();
    }

    /// <summary>
    /// CI only: Shell_NotifyIconGetRect answers only the icon's own process,
    /// so the tray job reads the icon's rectangle (it moves when the 「^」
    /// overflow opens) from a file written here.
    /// </summary>
    private void StartIconFile(string file, DispatcherQueue dispatcher)
    {
        iconFileTimer = dispatcher.CreateTimer();
        iconFileTimer.Interval = TimeSpan.FromMilliseconds(500);
        iconFileTimer.Tick += (_, _) =>
        {
            var r = tray?.TryGetIconRect();
            try
            {
                File.WriteAllText(file, r is Win32.RECT x ? $"{x.Left} {x.Top} {x.Width} {x.Height}" : "none");
            }
            catch (IOException)
            {
                // The test script may be reading it; next tick writes again.
            }
        };
        iconFileTimer.Start();
    }

    private void Quit()
    {
        if (quitting) return;
        quitting = true;
        ShellLog.Write("quit");
        iconFileTimer?.Stop();
        flyout?.Hide("quit", auto: false);
        flyout?.Dispose();
        if (settings != null)
        {
            settings.AllowClose = true;
            settings.Close();
        }
        tray?.Dispose();
        var watch = Stopwatch.StartNew();
        try
        {
            model?.Dispose();
        }
        catch (Exception ex)
        {
            ShellLog.Write($"quit: backend close failed {ex.GetType().Name}");
        }
        ShellLog.Write($"quit: backend closed in {watch.ElapsedMilliseconds} ms");
        if (panel != null)
        {
            panel.AllowClose = true;
            panel.Close();
        }
        Exit();
    }

    /// <summary>
    /// Shutdown or sign-out: same order as Quit, but never hold the session up.
    /// The backend gets 3 s to end its children; past that the process exits and
    /// the backend's job object ends whatever is left.
    /// </summary>
    private void EndSession()
    {
        if (quitting) return;
        quitting = true;
        ShellLog.Write("session ending");
        iconFileTimer?.Stop();
        flyout?.Hide("session-end", auto: false);
        flyout?.Dispose();
        settings?.AppWindow.Hide();
        tray?.Dispose();
        var close = Task.Run(() => model?.Dispose());
        bool closed = close.Wait(TimeSpan.FromSeconds(3));
        ShellLog.Write($"session ending: backend closed={closed}");
        Environment.Exit(0);
    }
}
