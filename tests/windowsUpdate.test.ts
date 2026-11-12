import { describe, expect, test } from "vitest";
import { readFileSync } from "node:fs";
import {
  type UpdatePhase,
  createUpdateRunner,
  phaseForNewOffer,
  updateButtonEnabled,
  updateButtonLabel,
  updateFailureReason,
} from "../src/lib/updateButton";

// Static checks for the one-click update flow (docs/WINDOWS_UPDATE.md). The
// installer itself is only built by the release workflow on Windows, so these
// pin the contracts that a normal CI run can still see.

const hooks = readFileSync("src-tauri/windows/installer-hooks.nsh", "utf-8");
const updater = readFileSync("src-tauri/src/services/updater.rs", "utf-8");
const workflow = readFileSync(".github/workflows/release.yml", "utf-8");

describe("update offer (#36)", () => {
  test("a check without a newer version clears the stored offer and tells the panel", () => {
    expect(updater).toContain("record_check(&ctx.update_info, info.as_ref())");
    expect(updater).toContain('app.emit("update-cleared", ())');
    const api = readFileSync("src/lib/api.ts", "utf-8");
    const appState = readFileSync("src/state/AppStateContext.tsx", "utf-8");
    expect(api).toContain('listen("update-cleared"');
    // Wired through followUpdateOffer, which applies null on update-cleared
    // (tests/updateOffer.test.ts).
    expect(appState).toContain("onCleared: onUpdateCleared");
    expect(appState).toContain("setUpdateInfo,");
  });
});

describe("update switches", () => {
  test("the app and the installer hook pass the same switches", () => {
    expect(updater).toContain('pub const INSTALLER_ARGS: [&str; 3] = ["/P", "/UPDATE", "/R"];');
    expect(hooks).toContain(`Exec '"$EXEPATH" /P /UPDATE /R'`);
  });

  test("the hook takes over only with no arguments and falls back to the wizard", () => {
    expect(hooks).toContain("!define MUI_CUSTOMFUNCTION_GUIINIT VibeUsageUpgradeInPlace");
    expect(hooks).toMatch(/\$\{GetParameters\} \$0\s+StrCmp \$0 "" 0 vibe_upgrade_wizard/);
    // Product name at run time: the external-test build shares the hook.
    expect(hooks).not.toMatch(/Uninstall\\Vibe Usage/);
    expect(hooks).toContain("Uninstall\\$(^Name)");
  });

  test("the hook function uses nothing the template defines after including it", () => {
    const body = hooks.slice(
      hooks.indexOf("Function VibeUsageUpgradeInPlace"),
      hooks.indexOf("FunctionEnd"),
    );
    for (const later of [
      "${PRODUCTNAME}",
      "${VERSION}",
      "${UNINSTKEY}",
      "${MANUPRODUCTKEY}",
      "$PassiveMode",
      "$UpdateMode",
      "$WixMode",
    ]) {
      expect(body).not.toContain(later);
    }
  });
});

describe("release workflow", () => {
  test("only a pushed tag uploads to the GitHub Release", () => {
    expect(workflow).toContain(
      "if: github.event_name == 'push' && startsWith(github.ref, 'refs/tags/')",
    );
    expect(workflow).toContain(
      "VIBE_USAGE_BUILD_KIND: ${{ github.event_name == 'workflow_dispatch' && inputs.kind || 'release' }}",
    );
  });

  test("a candidate is named as one and gets no update manifest", () => {
    expect(workflow).toContain("Windows-Candidate-$($env:GITHUB_RUN_NUMBER)-Setup.exe");
    expect(workflow).toContain("format('windows-installer-candidate-{0}', github.run_number)");
    const candidateBranch = workflow.slice(
      workflow.indexOf('-eq "release-candidate"'),
      workflow.indexOf("} else {", workflow.indexOf('-eq "release-candidate"')),
    );
    expect(candidateBranch).not.toContain("generate-updater-manifest");
    expect(workflow).toContain(
      "if: github.event_name == 'workflow_dispatch' && env.VIBE_USAGE_BUILD_KIND == 'external-test'",
    );
  });
});

describe("footer update button", () => {
  test("shows progress, refuses clicks while busy, and offers a retry with the reason", () => {
    expect(updateButtonLabel({ kind: "idle" })).toBe("发现更新");
    expect(updateButtonLabel({ kind: "downloading" })).toBe("正在下载更新…");
    expect(updateButtonLabel({ kind: "launching" })).toBe("正在启动安装…");
    expect(updateButtonLabel({ kind: "failed", reason: "下载失败：timeout" })).toBe(
      "更新失败：下载失败：timeout，点此重试",
    );
    expect(updateButtonEnabled({ kind: "idle" })).toBe(true);
    expect(updateButtonEnabled({ kind: "downloading" })).toBe(false);
    expect(updateButtonEnabled({ kind: "launching" })).toBe(false);
    expect(updateButtonEnabled({ kind: "failed", reason: "x" })).toBe(true);
  });

  test("a failed install unlocks the button and the next click retries", async () => {
    const phases: UpdatePhase[] = [];
    let fail = true;
    let calls = 0;
    let finish!: () => void;
    const install = () => {
      calls += 1;
      if (fail) return Promise.reject("下载失败：timeout");
      return new Promise<void>((resolve) => {
        finish = resolve;
      });
    };
    const click = createUpdateRunner(install, (phase) => phases.push(phase));

    await click();
    expect(calls).toBe(1);
    expect(phases).toEqual([
      { kind: "downloading" },
      { kind: "failed", reason: "下载失败：timeout" },
    ]);
    expect(updateButtonEnabled(phases[phases.length - 1])).toBe(true);

    fail = false;
    const retry = click();
    // A double click while the retry is still downloading starts nothing.
    await click();
    expect(calls).toBe(2);
    expect(updateButtonEnabled(phases[phases.length - 1])).toBe(false);

    finish();
    await retry;
    expect(phases.slice(2)).toEqual([{ kind: "downloading" }, { kind: "launching" }]);
    // The app is quitting for the installer: further clicks stay ignored.
    await click();
    expect(calls).toBe(2);
  });

  test("a withdrawn or replaced offer drops the old failure but not an install under way", () => {
    expect(phaseForNewOffer({ kind: "failed", reason: "下载失败：404" })).toEqual({ kind: "idle" });
    expect(phaseForNewOffer({ kind: "idle" })).toEqual({ kind: "idle" });
    expect(phaseForNewOffer({ kind: "downloading" })).toEqual({ kind: "downloading" });
    expect(phaseForNewOffer({ kind: "launching" })).toEqual({ kind: "launching" });
  });

  test("uses the backend's message as the reason", () => {
    expect(updateFailureReason("安装包校验失败 (SHA-256 不匹配)")).toBe(
      "安装包校验失败 (SHA-256 不匹配)",
    );
    expect(updateFailureReason(new Error("启动安装器失败：x"))).toBe("启动安装器失败：x");
    expect(updateFailureReason("")).toBe("未知错误");
  });
});
