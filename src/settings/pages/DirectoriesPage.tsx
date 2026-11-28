// 数据目录（高级）: extra places sync scans besides each tool's default
// directory (macOS `SettingsDirectoriesPage`). The CLI owns these fields; the
// page only calls it (`config roots` / `add-root` / `remove-root`). macOS's
// single 「额外 Codex Home」 row is not on Windows yet (gap list G20): the
// isolated directories below cover the same need.

import { Minus, Plus } from "lucide-react";
import { useSettings, type ExtraRootSource } from "../context";
import { Card, IconButton, PageHeader, PageScroll, Row, RowLabel, SectionTitle, Separator, Spacer, Spinner } from "../controls";
import { ProviderIcon } from "../../components/ProviderIcon";
import type { RateLimitProvider } from "../../lib/types";
import { SETTINGS_PAGE_TITLES } from "../model";
import { T, TYPE } from "../theme";

const SOURCES: { id: ExtraRootSource; name: string; provider: RateLimitProvider }[] = [
  { id: "codex", name: "Codex", provider: "codex" },
  { id: "grok", name: "Grok", provider: "grok" },
  { id: "antigravity", name: "Antigravity / AGY", provider: "antigravity" },
];

export function DirectoriesPage() {
  const m = useSettings();
  return (
    <PageScroll>
      <PageHeader title={SETTINGS_PAGE_TITLES.directories} advanced />
      <div style={{ marginTop: -14 }}>
        <SectionTitle title="隔离运行时目录" />
      </div>
      <Card>
        {SOURCES.map((source, index) => (
          <div key={source.id} className="flex flex-col">
            {index > 0 && <Separator />}
            <RootGroup source={source} paths={m.extraRoots[source.id] ?? []} />
          </div>
        ))}
      </Card>
      {m.extraRootsError && (
        <span className="break-words" style={{ ...TYPE.caption, color: T.red, padding: "8px 2px 0" }}>
          {m.extraRootsError}
        </span>
      )}
    </PageScroll>
  );
}

function RootGroup({ source, paths }: { source: (typeof SOURCES)[number]; paths: string[] }) {
  const m = useSettings();
  return (
    <div className="flex flex-col">
      <Row>
        <span className="inline-flex shrink-0 items-center justify-center" style={{ width: 18, height: 18 }}>
          <ProviderIcon provider={source.provider} size={20} color={T.text} />
        </span>
        <RowLabel title={source.name} />
        <Spacer />
        {m.extraRootsBusy && <Spinner />}
        <IconButton label={`给 ${source.name} 添加目录…`} disabled={m.extraRootsBusy} onClick={() => m.addExtraRoot(source.id)}>
          <Plus size={14} strokeWidth={2} />
        </IconButton>
      </Row>
      {paths.length > 0 && (
        <div className="flex flex-col" style={{ gap: 6, padding: `0 ${T.rowTrailing}px 12px 42px` }}>
          {paths.map((path) => (
            <div
              key={path}
              className="flex items-center"
              style={{
                height: 26,
                gap: 8,
                padding: "0 4px 0 9px",
                borderRadius: 6,
                background: T.inset,
                boxShadow: `inset 0 0 0 1px ${T.line}`,
              }}
            >
              <PathText path={path} />
              <IconButton label="移除此目录" disabled={m.extraRootsBusy} onClick={() => m.removeExtraRoot(source.id, path)}>
                <Minus size={14} strokeWidth={2} />
              </IconButton>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

/** A path cut in the middle (macOS `.truncationMode(.middle)`): the folders
 *  that tell two sandboxes apart are at the end. Full path on hover. */
function PathText({ path }: { path: string }) {
  const tail = Math.min(28, Math.floor(path.length / 2));
  const style = { ...TYPE.mono, color: T.text };
  return (
    <span className="flex min-w-0 flex-1" title={path} style={style}>
      <span className="truncate whitespace-pre">{path.slice(0, path.length - tail)}</span>
      <span className="shrink-0 whitespace-pre">{path.slice(path.length - tail)}</span>
    </span>
  );
}
