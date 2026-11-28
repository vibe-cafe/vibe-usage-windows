namespace VibeUsage.Backend;

public abstract partial class BackendClient
{
    private readonly SynchronizationContext context;
    private readonly object queueLock = new();
    private readonly object delivery = new();
    private readonly Queue<Action> queue = new();
    private readonly List<Action<BackendEvent>> listeners = [];
    private bool posted;
    private bool stopped;

    protected BackendClient(SynchronizationContext? context) =>
        this.context = context ?? SynchronizationContext.Current ?? new SynchronizationContext();

    private sealed class Subscription(Action remove) : IDisposable
    {
        private Action? removeOnce = remove;
        public void Dispose() => Interlocked.Exchange(ref removeOnce, null)?.Invoke();
    }

    public IDisposable Subscribe(Action<BackendEvent> receive)
    {
        lock (queueLock)
        {
            ObjectDisposedException.ThrowIf(stopped, this);
            listeners.Add(receive);
        }
        return new Subscription(() => { lock (queueLock) listeners.Remove(receive); });
    }

    internal void Emit(BackendEvent value)
    {
        Action<BackendEvent>[] targets;
        lock (queueLock) targets = listeners.ToArray();
        Post(() =>
        {
            foreach (var target in targets)
            {
                lock (queueLock) if (!listeners.Contains(target)) continue;
                target(value);
            }
        });
    }

    private void Post(Action action)
    {
        lock (queueLock)
        {
            if (stopped) return;
            queue.Enqueue(action);
            if (posted) return;
            posted = true;
        }
        context.Post(_ => Drain(), null);
    }

    private void Drain()
    {
        lock (delivery)
        {
            while (true)
            {
                Action? next;
                lock (queueLock)
                {
                    if (!queue.TryDequeue(out next)) { posted = false; return; }
                }
                next();
            }
        }
    }

    public async Task<IDisposable> WatchAsync<T>(Func<Task<T>> read,
        Func<BackendEvent, (bool Changed, T Value)> select, Action<T> receive)
    {
        // Both the event and the initial read are applied on the same queue.
        bool newer = false;
        var subscription = Subscribe(e =>
        {
            var update = select(e);
            if (!update.Changed) return;
            newer = true;
            receive(update.Value);
        });
        try
        {
            T initial = await read().ConfigureAwait(false);
            bool detached = false;
            Post(() => { if (!newer && !detached) receive(initial); });
            return new Subscription(() =>
            {
                lock (delivery) { detached = true; subscription.Dispose(); }
            });
        }
        catch { subscription.Dispose(); throw; }
    }

    public void Dispose()
    {
        lock (delivery)
        lock (queueLock)
        {
            stopped = true;
            queue.Clear();
            listeners.Clear();
        }
        CloseTransport();
    }

    protected abstract void CloseTransport();
}

public static class BackendWatches
{
    public static Task<IDisposable> WatchUpdatesAsync(this IBackend app, Action<UpdateInfo?> receive) =>
        app.WatchAsync(app.GetUpdateInfoAsync, e => e switch
        {
            BackendEvent.UpdateAvailable next => (true, next.Value),
            BackendEvent.UpdateCleared => (true, null),
            _ => (false, null)
        }, receive);

    public static Task<IDisposable> WatchQuotasAsync(this IBackend app, Action<QuotaCache> receive) =>
        app.WatchAsync(app.GetCachedRateLimitsAsync, e => e is BackendEvent.QuotasChanged next
            ? (true, next.Value) : (false, default!), receive);
}
