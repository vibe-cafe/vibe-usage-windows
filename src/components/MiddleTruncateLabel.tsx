// Middle truncation, mirroring macOS `.truncationMode(.middle)` (DistributionChartsView):
// same-prefix names (org/repo-a vs org/repo-b) only differ at the tail, so the head
// span truncates with an ellipsis while the tail span never shrinks. Full name shows
// via native title tooltip on hover.
//
// Both halves are flex items, and a flex item drops whitespace at its own edges,
// so a space at the cut ("Gemini 4 " + "Carbon") used to vanish and render as
// "Gemini 4Carbon". `whitespace-pre` keeps it; the line still never wraps, so the
// head's ellipsis works as before. Same fix as the web dashboard's LegendLabel.

import { CSSProperties } from "react";

const TAIL_CHARS = 6;

export function MiddleTruncateLabel({
  label,
  className,
  style,
}: {
  label: string;
  className?: string;
  style?: CSSProperties;
}) {
  if (label.length <= TAIL_CHARS) {
    return (
      <span className={`truncate ${className ?? ""}`} style={style} title={label}>
        {label}
      </span>
    );
  }
  return (
    <span className={`flex min-w-0 ${className ?? ""}`} style={style} title={label}>
      <span className="truncate whitespace-pre">{label.slice(0, -TAIL_CHARS)}</span>
      <span className="shrink-0 whitespace-pre">{label.slice(-TAIL_CHARS)}</span>
    </span>
  );
}
