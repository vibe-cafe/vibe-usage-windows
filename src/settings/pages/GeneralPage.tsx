// 通用: what the app shows in the tray and how it starts (macOS
// `SettingsGeneralPage`). macOS's menu-bar preview and 「在 Dock 中显示」 have
// no Windows counterpart; the menu-bar switches are the tray's here
// (docs/PARITY.md 平台差异 1, 2 and 9).

import { useSettings } from "../context";
import { Card, PageHeader, PageScroll, Row, RowLabel, Separator, Spacer, Switch } from "../controls";
import { SETTINGS_PAGE_TITLES } from "../model";
import { T } from "../theme";

export function GeneralPage() {
  const { settings, patchSettings, autoStart, setAutoStart } = useSettings();
  return (
    <PageScroll>
      <PageHeader title={SETTINGS_PAGE_TITLES.general} />
      <Card>
        <Row>
          <RowLabel title="托盘显示费用" />
          <Spacer />
          <Switch
            label="托盘显示费用"
            checked={settings?.showCostInTray ?? true}
            disabled={!settings}
            onChange={(value) => patchSettings({ showCostInTray: value })}
          />
        </Row>
        <Separator />
        <Row>
          <RowLabel title="托盘显示 Token" />
          <Spacer />
          <Switch
            label="托盘显示 Token"
            checked={settings?.showTokensInTray ?? false}
            disabled={!settings}
            onChange={(value) => patchSettings({ showTokensInTray: value })}
          />
        </Row>
      </Card>
      <Card style={{ marginTop: T.cardGap }}>
        <Row>
          <RowLabel title="开机自启动" />
          <Spacer />
          <Switch label="开机自启动" checked={autoStart} onChange={setAutoStart} />
        </Row>
      </Card>
    </PageScroll>
  );
}
