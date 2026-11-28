// 关于, laid out like macOS `SettingsAboutPage`: an identity card (icon,
// name, version), then 更新, 诊断 and 链接 groups, and the copyright line.
// 更新 keeps the Windows updater's own flow (docs/PARITY.md 平台差异 5): the
// check runs here, 「安装更新」 downloads and installs without a second
// question. 「导出诊断日志」 (测试版) appears in test builds only.

import { ArrowUpRight, Copy, Globe } from "lucide-react";
import { useState, type ReactNode } from "react";
import { useSettings } from "../context";
import {
  AppIcon,
  Button,
  Card,
  CheckMark,
  IconLabel,
  PageHeader,
  PageScroll,
  Row,
  RowLabel,
  Section,
  Separator,
  Spacer,
  Spinner,
  Tag,
} from "../controls";
import { type UpdatePhase, updateButtonEnabled } from "../../lib/updateButton";
import { SETTINGS_LINKS, SETTINGS_PAGE_TITLES, settingsVersionText, siteUrl } from "../model";
import { T, TYPE } from "../theme";

export function AboutPage() {
  const m = useSettings();
  const version = settingsVersionText(m.status);
  const site = siteUrl(m.status?.apiUrl);
  return (
    <PageScroll>
      <PageHeader title={SETTINGS_PAGE_TITLES.about} />
      <Card>
        <div className="flex flex-col items-center" style={{ padding: "18px 0" }}>
          <AppIcon size={68} />
          <span style={{ ...TYPE.pageTitle, color: T.text, paddingTop: 8 }}>{m.appName}</span>
          <span style={{ ...TYPE.caption, color: T.secondaryText, paddingTop: 3 }}>{version && `版本 ${version}`}</span>
        </div>
      </Card>

      <Section title="更新">
        <UpdateRow version={version} />
      </Section>

      <Section title="诊断">
        <DiagnosticsRows />
      </Section>

      <Section title="链接">
        <LinkRow title="vibe-usage" url={SETTINGS_LINKS.cliRepository} mark={<GitHubMark />} />
        <Separator />
        <LinkRow title="vibe-usage-windows" url={SETTINGS_LINKS.appRepository} mark={<GitHubMark />} />
        <Separator />
        <LinkRow title="VibeCafé" url={site} mark={<Globe size={15} strokeWidth={1.7} />} />
      </Section>

      <span className="text-center" style={{ ...TYPE.caption, color: T.tertiaryText, paddingTop: 18 }}>
        © 2026 VibeCafé · MIT 许可证
      </span>
    </PageScroll>
  );
}

function UpdateRow({ version }: { version: string }) {
  const m = useSettings();
  const failed = m.updatePhase.kind === "failed" ? m.updatePhase.reason : null;
  const text =
    failed != null
      ? `更新失败：${failed}`
      : (m.updateMessage ?? (m.updateOffer ? `发现新版本 ${m.updateOffer.version}` : null));
  return (
    <Row>
      <span className="shrink-0">
        <RowLabel title={version && `版本 ${version}`} />
      </span>
      <Spacer />
      {m.status?.updatesAvailable ? (
        <>
          {text && (
            <span className="min-w-0 truncate" title={text} style={{ ...TYPE.body, color: failed != null ? T.red : T.secondaryText }}>
              {text}
            </span>
          )}
          <Button disabled={!updateButtonEnabled(m.updatePhase)} onClick={m.checkUpdate}>
            检查更新
          </Button>
          {m.updateOffer && (
            <Button disabled={!updateButtonEnabled(m.updatePhase)} onClick={m.installUpdate}>
              {installLabel(m.updatePhase)}
            </Button>
          )}
        </>
      ) : m.status ? (
        <span style={{ ...TYPE.body, color: T.secondaryText }}>外测版不检查更新</span>
      ) : null}
    </Row>
  );
}

function installLabel(phase: UpdatePhase): string {
  switch (phase.kind) {
    case "idle":
      return "安装更新";
    case "downloading":
      return "正在下载…";
    case "launching":
      return "正在启动安装…";
    case "failed":
      return "重试";
  }
}

/** The button copies a whitelisted summary (no raw output, paths, keys or
 *  account names); what was copied stays readable underneath. */
function DiagnosticsRows() {
  const m = useSettings();
  return (
    <>
      <Row tall>
        <RowLabel
          title="复制诊断信息"
          caption={m.diagnosticsMessage}
          captionColor={m.diagnosticsMessage?.startsWith("收集失败") ? T.red : T.secondaryText}
        />
        <Spacer />
        <Button disabled={m.diagnosticsBusy} onClick={m.copyDiagnostics}>
          {m.diagnosticsBusy ? (
            <span className="inline-flex items-center" style={{ gap: 5 }}>
              <Spinner />
              复制
            </span>
          ) : m.diagnosticsCopied ? (
            <span className="inline-flex items-center" style={{ gap: 5 }}>
              <CheckMark />
              已复制
            </span>
          ) : (
            <IconLabel icon={<Copy size={12} strokeWidth={2} />} title="复制" />
          )}
        </Button>
      </Row>
      {m.diagnosticsText && (
        <div style={{ padding: `0 ${T.rowTrailing}px 12px ${T.rowLeading}px` }}>
          <pre
            className="settings-scroll m-0 overflow-auto whitespace-pre-wrap break-all"
            style={{
              maxHeight: 150,
              padding: "9px 10px",
              borderRadius: 6,
              background: T.inset,
              boxShadow: `inset 0 0 0 1px ${T.line}`,
              fontFamily: T.mono,
              fontSize: 11,
              lineHeight: "17px",
              color: T.secondaryText,
              userSelect: "text",
            }}
          >
            {m.diagnosticsText}
          </pre>
        </div>
      )}
      {m.status?.testDiagnosticsAvailable && (
        <>
          <Separator />
          <Row tall={m.testExportMessage != null}>
            <span className="flex min-w-0 flex-col" style={{ gap: 1 }}>
              <span className="flex items-center" style={{ gap: 8 }}>
                <span style={{ ...TYPE.label, color: T.text }}>导出诊断日志</span>
                <Tag kind="testBuild" />
              </span>
              {m.testExportMessage && (
                <span style={{ ...TYPE.caption, color: T.secondaryText }}>{m.testExportMessage}</span>
              )}
            </span>
            <Spacer />
            <Button onClick={m.exportTestDiagnostics}>导出…</Button>
          </Row>
        </>
      )}
    </>
  );
}

/** One row of the 链接 card: a monochrome mark, the name, a grey ↗ at the
 *  end. The whole row opens the link in the default browser and lightens
 *  slightly under the pointer; no framed button. */
function LinkRow({ title, url, mark }: { title: string; url: string | null; mark: ReactNode }) {
  const { openExternal } = useSettings();
  const [hovered, setHovered] = useState(false);
  return (
    <button
      type="button"
      disabled={!url}
      title={url ?? undefined}
      onClick={() => url && openExternal(url)}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      className="settings-focus flex w-full items-center text-left"
      style={{
        gap: 10,
        minHeight: T.rowHeight,
        padding: "0 14px",
        color: T.text,
        background: hovered && url ? "rgba(255,255,255,0.04)" : "transparent",
      }}
    >
      <span className="inline-flex shrink-0 items-center justify-center" style={{ width: 18, height: 18 }}>
        {mark}
      </span>
      <span style={TYPE.label}>{title}</span>
      <Spacer />
      <ArrowUpRight size={13} strokeWidth={2} color={T.tertiaryText} />
    </button>
  );
}

/** The GitHub mark, Simple Icons `github.svg` (CC0 1.0), the same path the
 *  macOS app draws; a GitHub trademark, used only to label links to GitHub. */
function GitHubMark() {
  return (
    <svg aria-hidden="true" width={15} height={15} viewBox="0 0 24 24" fill="currentColor" fillRule="evenodd">
      <path d="M12 .297c-6.63 0-12 5.373-12 12 0 5.303 3.438 9.8 8.205 11.385.6.113.82-.258.82-.577 0-.285-.01-1.04-.015-2.04-3.338.724-4.042-1.61-4.042-1.61C4.422 18.07 3.633 17.7 3.633 17.7c-1.087-.744.084-.729.084-.729 1.205.084 1.838 1.236 1.838 1.236 1.07 1.835 2.809 1.305 3.495.998.108-.776.417-1.305.76-1.605-2.665-.3-5.466-1.332-5.466-5.93 0-1.31.465-2.38 1.235-3.22-.135-.303-.54-1.523.105-3.176 0 0 1.005-.322 3.3 1.23.96-.267 1.98-.399 3-.405 1.02.006 2.04.138 3 .405 2.28-1.552 3.285-1.23 3.285-1.23.645 1.653.24 2.873.12 3.176.765.84 1.23 1.91 1.23 3.22 0 4.61-2.805 5.625-5.475 5.92.42.36.81 1.096.81 2.22 0 1.606-.015 2.896-.015 3.286 0 .315.21.69.825.57C20.565 22.092 24 17.592 24 12.297c0-6.627-5.373-12-12-12" />
    </svg>
  );
}
