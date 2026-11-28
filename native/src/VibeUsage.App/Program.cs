using System;
using System.Threading;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using VibeUsage.App.Shell;

namespace VibeUsage.App;

public static class Program
{
    /// <summary>
    /// Single instance before WinUI starts: a second start hands over to the
    /// running app (or, with --autostart, just exits) without creating any window.
    /// </summary>
    [STAThread]
    private static void Main(string[] args)
    {
        var options = LaunchOptions.Parse(args);
        ShellLog.UseDirectory(options.LogDir);
        using var instance = new SingleInstance();
        if (!instance.IsFirst)
        {
            SingleInstance.HandOff(options.Autostart);
            return;
        }

        // A start by the user opens the panel with the keyboard, but Windows only
        // lets a just-started process take the foreground early on, and WinUI's
        // start-up uses that up before the panel exists. Pass the right to this
        // process now, as a second start passes it to the running one.
        if (!options.Autostart)
        {
            bool kept = Win32.AllowSetForegroundWindow((uint)Environment.ProcessId);
            ShellLog.Write($"start by the user: foreground right kept={kept}");
        }

        WinRT.ComWrappersSupport.InitializeComWrappers();
        Application.Start(callback =>
        {
            var context = new DispatcherQueueSynchronizationContext(DispatcherQueue.GetForCurrentThread());
            SynchronizationContext.SetSynchronizationContext(context);
            _ = new ShellApp(options);
        });
    }
}
