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

        WinRT.ComWrappersSupport.InitializeComWrappers();
        Application.Start(callback =>
        {
            var context = new DispatcherQueueSynchronizationContext(DispatcherQueue.GetForCurrentThread());
            SynchronizationContext.SetSynchronizationContext(context);
            _ = new ShellApp(options);
        });
    }
}
