using System.Collections.Concurrent;
using System.Runtime.InteropServices;
using System.Text.Json;

namespace VibeUsage.Backend;

public sealed class NativeBackend : BackendClient
{
    public const uint InterfaceVersion = 1;
    private readonly object admission = new();
    private readonly Func<BackendError?> close;
    private readonly ConcurrentDictionary<ulong, TaskCompletionSource<JsonElement>> pending = new();
    private GCHandle callbackRoot;
    private ulong lastId;
    private bool closed;
    public BackendError? CloseError { get; private set; }

    public NativeBackend(HostOptions options, SynchronizationContext? context = null)
        : this(options, InterfaceVersion, context) { }

    internal NativeBackend(HostOptions options, uint version, SynchronizationContext? context = null,
        Func<uint, string, Native.Callback, nint>? init = null, Func<BackendError?>? close = null) : base(context)
    {
        this.close = close ?? (() => ReadError(Native.Close()));
        Native.Callback callback = Receive;
        callbackRoot = GCHandle.Alloc(callback);
        try { Check((init ?? Native.Init)(version, Wire.Element(options).GetRawText(), callback)); }
        catch { callbackRoot.Free(); throw; }
    }

    internal override Task<JsonElement> SendAsync(string name, JsonElement args)
    {
        lock (admission)
        {
            ObjectDisposedException.ThrowIf(closed, this);
            ulong id = ++lastId;
            var result = new TaskCompletionSource<JsonElement>(TaskCreationOptions.RunContinuationsAsynchronously);
            pending[id] = result;
            try
            {
                var accepted = Native.Call(id, name, args.GetRawText());
                Check(accepted.Error);
                if (accepted.RequestId != id) throw new InvalidDataException("后端返回的请求号不匹配");
            }
            catch { pending.TryRemove(id, out _); throw; }
            return result.Task;
        }
    }

    // Do not run host code or take the admission lock on a Rust thread.
    private void Receive(nint pointer)
    {
        TaskCompletionSource<JsonElement>? completion = null;
        try
        {
            using var doc = JsonDocument.Parse(Take(pointer));
            var msg = doc.RootElement;
            if (msg.GetProperty("kind").GetString() == "event")
            {
                Emit(BackendEvent.Parse(msg.GetProperty("name").GetString()!, msg.GetProperty("payload")));
                return;
            }
            if (!pending.TryRemove(msg.GetProperty("requestId").GetUInt64(), out completion)) return;
            if (msg.TryGetProperty("error", out var error))
                completion.SetException(new BackendException(Wire.Read<BackendError>(error)));
            else completion.SetResult(msg.GetProperty("value").Clone());
        }
        catch (Exception error)
        {
            // A marshalling/contract failure must not escape the reverse P/Invoke.
            completion?.TrySetException(error);
        }
    }

    private static string Take(nint pointer)
    {
        try { return Marshal.PtrToStringUTF8(pointer)!; }
        finally { Check(Native.Free(pointer)); }
    }

    internal static BackendError? ReadError(nint error)
    {
        if (error == 0) return null;
        using var doc = JsonDocument.Parse(Take(error));
        return Wire.Read<BackendError>(doc.RootElement);
    }

    private static void Check(nint error)
    {
        if (ReadError(error) is { } failure) throw new BackendException(failure);
    }

    protected override void CloseTransport()
    {
        lock (admission)
        {
            if (!callbackRoot.IsAllocated) return;
            closed = true;
            CloseError = close();
            // A close rejected on the callback stack leaves the native session live.
            // Keep the delegate rooted until Dispose on the host thread finishes.
            if (CloseError?.Code != "closeFromRuntime") callbackRoot.Free();
        }
    }

    internal static class Native
    {
        private const string Library = "vibe_ffi";
        [StructLayout(LayoutKind.Sequential)]
        internal struct Admission { public ulong RequestId; public nint Error; }
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        internal delegate void Callback(nint json);
        [DllImport(Library, EntryPoint = "vibe_init", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern nint Init(uint version, [MarshalAs(UnmanagedType.LPUTF8Str)] string options, Callback callback);
        [DllImport(Library, EntryPoint = "vibe_call", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern Admission Call(ulong id, [MarshalAs(UnmanagedType.LPUTF8Str)] string name,
            [MarshalAs(UnmanagedType.LPUTF8Str)] string args);
        [DllImport(Library, EntryPoint = "vibe_close", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern nint Close();
        [DllImport(Library, EntryPoint = "vibe_free", CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        internal static extern nint Free(nint json);
    }
}
