using System;
using System.Threading;

namespace VibeUsage.App.Shell;

/// <summary>
/// One app per user session: a named mutex. A second start finds the running
/// app's tray host window, lets it take the foreground (this process was just
/// started by the user, so it holds that right and can pass it on), asks it
/// to open the panel, and exits. A late start from the sign-in entry
/// (--autostart) exits without opening anything. The external test build
/// (#93) gets its own mutex and window class.
/// </summary>
internal sealed class SingleInstance : IDisposable
{
    private const string MutexName = @"Local\VibeUsage.Native.SingleInstance";
    private readonly Mutex mutex;

    public bool IsFirst { get; }

    public SingleInstance()
    {
        mutex = new Mutex(true, MutexName, out bool created);
        IsFirst = created;
    }

    /// <summary>Hands the start over to the running app. Waits up to 5 s for its tray host while it is still starting.</summary>
    public static void HandOff(bool autostart)
    {
        if (autostart)
        {
            ShellLog.Write("second start with --autostart: already running, nothing to open");
            return;
        }
        IntPtr host = IntPtr.Zero;
        for (int i = 0; i < 50 && host == IntPtr.Zero; i++)
        {
            host = Win32.FindWindowW(TrayIcon.HostClassName, null);
            if (host == IntPtr.Zero) Thread.Sleep(100);
        }
        if (host == IntPtr.Zero)
        {
            ShellLog.Write("second start: running app's tray host not found");
            return;
        }
        bool allowed = Win32.AllowSetForegroundWindow(Win32.ProcessOf(host));
        bool posted = Win32.PostMessageW(host, TrayIcon.WM_OPEN_REQUEST, 1, IntPtr.Zero);
        ShellLog.Write($"second start: asked the running app to open the panel (foreground passed={allowed}, posted={posted})");
    }

    public void Dispose()
    {
        try
        {
            if (IsFirst) mutex.ReleaseMutex();
        }
        catch (ApplicationException)
        {
            // Not held by this thread any more; disposing still closes the handle.
        }
        mutex.Dispose();
    }
}
