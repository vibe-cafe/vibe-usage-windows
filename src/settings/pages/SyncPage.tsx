// 数据同步: the account key and re-linking, sync health with the last ten
// runs, and the reset (macOS `SettingsSyncPage`). The reset asks first in an
// in-window confirmation (`SettingsApp`), like macOS's system alert.

import { useSettings } from "../context";
import { Button, Card, PageHeader, PageScroll, Row, RowLabel, Separator, Spacer, Spinner, StatusDot } from "../controls";
import { SETTINGS_PAGE_TITLES, syncBars, syncLine } from "../model";
import { T, TYPE } from "../theme";

export function SyncPage() {
  const m = useSettings();
  const configured = m.status?.configured ?? false;
  const key = m.status?.apiKeyDisplay ?? null;
  return (
    <PageScroll>
      <PageHeader title={SETTINGS_PAGE_TITLES.sync} />
      <Card>
        <Row tall={m.relinkError != null}>
          <RowLabel title="API Key" caption={m.relinkError} captionColor={T.red} />
          <Spacer />
          {key ? (
            <span className="truncate" style={{ ...TYPE.mono, color: T.text }}>
              {key}
            </span>
          ) : (
            <span className="shrink-0" style={{ ...TYPE.body, color: T.secondaryText }}>
              未配置
            </span>
          )}
          {m.relinking ? (
            <Button onClick={m.cancelRelink}>取消</Button>
          ) : key ? (
            <Button onClick={m.startRelink}>重新链接</Button>
          ) : (
            <Button kind="primary" onClick={m.startRelink}>
              链接
            </Button>
          )}
        </Row>
        {m.relinking && m.relinkUserCode && <CodeBox code={m.relinkUserCode} />}
        <Separator />
        <SyncRow configured={configured} />
        <Separator />
        <Row tall>
          <RowLabel title="重置配置" />
          <Spacer />
          <Button kind="danger" onClick={m.askReset}>
            重置…
          </Button>
        </Row>
      </Card>
    </PageScroll>
  );
}

/** The device-flow code to compare with the browser, while it waits. */
function CodeBox({ code }: { code: string }) {
  return (
    <div style={{ padding: `0 ${T.rowTrailing}px 12px ${T.rowLeading}px` }}>
      <div
        className="flex items-center"
        style={{
          height: 40,
          gap: 10,
          padding: "0 12px",
          borderRadius: 8,
          background: T.inset,
          boxShadow: `inset 0 0 0 1px ${T.line}`,
        }}
      >
        <span style={{ ...TYPE.body, color: T.secondaryText }}>验证码</span>
        <span
          style={{
            fontFamily: T.mono,
            fontSize: 15,
            fontWeight: 600,
            letterSpacing: 1.2,
            color: T.text,
            userSelect: "text",
          }}
        >
          {code}
        </span>
        <span className="grow" />
        <Spinner />
        <span style={{ ...TYPE.body, color: T.secondaryText }}>等浏览器里确认…</span>
      </div>
    </div>
  );
}

function SyncRow({ configured }: { configured: boolean }) {
  const { syncState, syncHistory } = useSettings();
  const line = syncLine(syncState, configured);
  const bars = configured ? syncBars(syncHistory) : [];
  const color = line.kind === "error" ? T.red : configured ? T.text : T.tertiaryText;
  return (
    <Row>
      <RowLabel title="同步" />
      <Spacer />
      <span className="flex min-w-0 items-center" style={{ gap: 7 }}>
        {bars.length > 0 && <SyncBars bars={bars} />}
        {line.spinning && <Spinner />}
        {line.kind && <StatusDot kind={line.kind} />}
        <span className="truncate" title={line.text} style={{ ...TYPE.body, color }}>
          {line.text}
        </span>
      </span>
    </Row>
  );
}

/** The last ten syncs as thin bars, oldest first: green a success, red a
 *  failure; the newest at full strength. From the app's own local sync
 *  history; nothing new is recorded for it. */
function SyncBars({ bars }: { bars: boolean[] }) {
  const failures = bars.filter((success) => !success).length;
  return (
    <span
      role="img"
      aria-label={`最近 ${bars.length} 次同步，失败 ${failures} 次`}
      title={`最近 ${bars.length} 次同步`}
      className="flex shrink-0 items-center"
      style={{ gap: 2, paddingRight: 4 }}
    >
      {bars.map((success, index) => {
        const newest = index === bars.length - 1;
        return (
          <span
            key={index}
            style={{
              width: 3,
              height: 11,
              borderRadius: 1.5,
              background: success ? T.ok : T.red,
              opacity: newest ? 1 : success ? 0.6 : 0.95,
            }}
          />
        );
      })}
    </span>
  );
}
