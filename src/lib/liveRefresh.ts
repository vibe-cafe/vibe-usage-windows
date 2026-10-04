/** Refresh visible panels without overlapping ticks or work after disposal. */
export function startLiveRefresh(refresh: () => Promise<unknown>, isVisible: () => Promise<boolean>): () => void {
  let stopped = false;
  let pending = false;
  const timer = setInterval(() => {
    if (stopped || pending) return;
    pending = true;
    void (async () => {
      if (await isVisible() && !stopped) await refresh();
    })().catch(() => {
      // Retry on the next tick after transient IPC/network errors.
    }).finally(() => { pending = false; });
  }, 60_000);
  return () => { stopped = true; clearInterval(timer); };
}
