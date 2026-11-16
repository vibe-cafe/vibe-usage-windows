import { RefObject, useLayoutEffect, useState } from "react";

/**
 * Live content width of `ref`'s element (0 before it mounts). The main window
 * is resizable, so layout that is computed in pixels has to follow it instead
 * of reading the width once at render time.
 */
export function useElementWidth(ref: RefObject<HTMLElement>): number {
  const [width, setWidth] = useState(0);
  useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return;
    setWidth(element.clientWidth);
    const observer = new ResizeObserver((entries) => {
      const entry = entries[entries.length - 1];
      if (entry) setWidth(entry.contentRect.width);
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, [ref]);
  return width;
}
