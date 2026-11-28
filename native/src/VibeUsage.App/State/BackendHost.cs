using System;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using VibeUsage.App.Shell;
using VibeUsage.Backend;

namespace VibeUsage.App.State;

/// <summary>
/// Picks the backend: the Rust DLL (<c>vibe_ffi.dll</c> next to the exe) by
/// default, the in-memory fake with <c>--fake-backend</c>. The DLL gets the
/// app's two data folders, the install folder (where the CLI launcher and the
/// bundled Node sit, as in the Tauri layout), the version and whether this is
/// a sign-in start; it does the start-up sequence itself (#85). When the DLL
/// is missing or refuses to start, the app keeps running with a backend that
/// fails every command: the panel stays empty instead of showing made-up data.
/// </summary>
internal static class BackendHost
{
    public static IBackend Create(LaunchOptions options, SynchronizationContext context)
    {
        if (options.FakeBackend)
        {
            ShellLog.Write($"backend: fake ({options.Demo})");
            return DemoBackend.Create(options.Demo, context);
        }
        var (roaming, local) = options.DataFolders(AppPaths.RoamingConfig, AppPaths.LocalData);
        var host = new HostOptions(roaming, local, AppContext.BaseDirectory, AppVersion, options.Autostart);
        try
        {
            var backend = new NativeBackend(host, context);
            ShellLog.Write($"backend: native interface v{NativeBackend.InterfaceVersion} version={AppVersion} autostart={options.Autostart} " +
                           $"testData={options.DataDir != null}");
            return backend;
        }
        catch (Exception ex) when (ex is DllNotFoundException or EntryPointNotFoundException or BadImageFormatException
                                       or BackendException or JsonException)
        {
            ShellLog.Write($"backend: native unavailable ({(ex is BackendException b ? b.Code : ex.GetType().Name)})");
            var unavailable = new FakeBackend(context);
            unavailable.Respond = (_, _) => Task.FromException<JsonElement>(new BackendException(new("unavailable", "后端不可用")));
            return unavailable;
        }
    }

    /// <summary>Major.minor.patch of this build (the backend compares it with the update manifest).</summary>
    public static string AppVersion
    {
        get
        {
            var v = typeof(BackendHost).Assembly.GetName().Version;
            return v == null ? "0.0.0" : $"{v.Major}.{v.Minor}.{Math.Max(0, v.Build)}";
        }
    }
}
