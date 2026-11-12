// The footer's 「发现更新」 offer. The backend checks for an update right at
// startup and then once a day, announces a find with `update-available` and a
// withdrawn one with `update-cleared` (#36). That first check can finish
// before this window has attached its listeners; the event is then lost and
// the offer would only appear after the next check, a day later. So once both
// listeners are attached, read what the backend already holds — unless either
// event has arrived meanwhile, which is newer than the read.

import type { UnlistenFn } from "@tauri-apps/api/event";
import type { UpdateInfo } from "./types";

export interface UpdateOfferSource {
  onAvailable: (handler: (info: UpdateInfo) => void) => Promise<UnlistenFn>;
  onCleared: (handler: () => void) => Promise<UnlistenFn>;
  /** The backend's latest result; never starts a check. */
  readCurrent: () => Promise<UpdateInfo | null>;
}

export async function followUpdateOffer(
  source: UpdateOfferSource,
  apply: (info: UpdateInfo | null) => void,
): Promise<UnlistenFn> {
  let heard = false;
  const unlisten = await Promise.all([
    source.onAvailable((info) => {
      heard = true;
      apply(info);
    }),
    source.onCleared(() => {
      heard = true;
      apply(null);
    }),
  ]);
  try {
    const current = await source.readCurrent();
    if (current && !heard) apply(current);
  } catch {
    // Nothing to show yet; the next check's event still arrives.
  }
  return () => {
    for (const stop of unlisten) stop();
  };
}
