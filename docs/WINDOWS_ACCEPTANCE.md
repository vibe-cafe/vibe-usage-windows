# Windows follow-up acceptance — exit cleanup and build environment

Mac handles integration; Windows verifies native behavior. Both repositories use
`fix/windows-acceptance`. Clone the supplied Git bundles and verify all hashes
and both actual HEADs against MANIFEST.json before testing. The Windows bundle
is a source snapshot; `sourceCommit` is the original Mac development commit.
Diagnostics must record the actual cloned snapshot HEAD.

## Accepted evidence and preserved failures

The complete 0.5.13 return was received and all **64 checksum targets** verified,
including the installer and 12 cropped screenshots. CLI `5387113efc20` / Windows
snapshot `14c7ccb5bf29` / development `458feb52202b` was tested on Windows:

- CLI and actual vendored CLI: 377 total / 367 pass / 0 fail / 10 explained skips.
  All eight originally failing cases executed successfully, including ACLs.
- Both real 168-character-source release builds passed: automatic short Cargo
  target and explicit target override. Source/Vite cwd remained unchanged.
- Installed 0.5.13 / windows-acceptance-1789260418 identity and Codex/Kimi positive
  quotas match diagnostics/screenshots. GUI, installation and final restoration
  evidence has now been reviewed on Mac. This is evidence review, not a second
  Windows execution. Dropdown screenshots alone do not establish numerical
  correctness of every filter/date combination.
- Preserve raw Cargo LNK1104/101, initial PSModulePath ACL failures and initial
  missing-pnpm failure. Preserve the first uninstall's leftover node.exe/empty
  CLI directories; two later clean uninstalls do not erase it.
- The much older CLI 4ab7b98 result (359 pass / 8 fail / 10 skip) remains a failed
  historical baseline. Do not rewrite it using later passes.

## New revision

App **0.5.12**, CLI **0.11.0**, unpublished. This revision has not yet run on
Windows. Read package.json for the app version. That revision predates the
current CLI model: the app no longer carries a pinned snapshot — it resolves
`@vibe-cafe/vibe-usage@latest` at run time through
`src-tauri/resources/cli-bootstrap.mjs`.

- LONGPATH-CARGO-01: `scripts/cargo-windows.ps1` applies the same stable short
  target policy to development Cargo commands, forwards arguments and exit
  codes, preserves explicit targets and restores the caller environment.
  Cargo has no dynamic pre-command hook: an unconfigured bare `cargo test`
  still has the native Windows long-path limitation. This is documented and
  mitigated through the supported entry, not claimed to be fixed in Cargo.
- UNINSTALL-RACE-01: managed CLI/probe children start suspended, are assigned
  to an unnamed Windows Job Object, then resume. Descendants cannot escape
  before assignment. App quit closes the spawn gate, terminates only its owned
  jobs and waits up to 5 seconds for them to empty. Job handles also kill their
  members when the app is forcibly terminated. Browser and updater launchers
  are outside this scope. Scope drop cancels descendants on timeout as well.
  NSIS retries only the packaged node.exe for 3 seconds, then uses /REBOOTOK;
  it removes only known empty CLI directories, never shared config/credentials.
  Deferred deletion reports exit 3010 (reboot required); do not reinstall into
  that directory before the pending deletion is completed.
  This fixes a source-level cleanup gap consistent with the report; the first
  failure's exact root cause is not proven by the available logs.
- PSMODULEPATH-01: the ACL test child chooses its own Windows PowerShell inbox
  modules, so a PS7 host's module paths do not load incompatible Security DLLs.
  Do not manually clean PSModulePath for the new test; exercise the bad host.
- TOOL-PATH-01: a pnpm run child recovers existing pnpm shims from npm_execpath
  and keeps npm_node_execpath's Node first on PATH. No auto-download of pnpm or
  npm latest is added. If no usable installation exists, fail with instructions.
- CLI-VERSION-01: `--version` and `-v` print the version from the shipped
  package.json and exit successfully without reading config or starting sync.
- PNPM-LONGPATH-01: `.pnpmfile.cjs` uses a short Windows virtual store keyed by
  the canonical checkout path (pnpm >=10.8, <11). Explicit virtual-store overrides
  are authoritative; other platforms keep pnpm defaults. Regress A install/build,
  B production install, A build again, concurrent builds and explicit overrides.
- RELEASE-PRIVACY-01: the supported Tauri build wrapper remaps source/home paths
  for rustc, including dependencies, and restores plain/encoded flags on failure.
  Scan installed EXE payloads, not only compressed setup files; preserve failures.
- QUOTA-SELECTOR-01: display discovery separately from a matching quota result;
  never equate an unread product with no-data, or use another provider's snapshot.
- QUOTA-PRESENTATION-01: every enabled product always renders its own card, in
  selection order, in one horizontally scrolling row; the section never folds
  into a generic line, and a third selection never evicts the first. Empty copy
  is graded by source and never guesses: refresh in flight, the live Codex
  endpoint's own `allowed:false` / `limit_reached:true` verdict, the same
  endpoint answering without a window, local detection with nothing read yet,
  and no local install/login. Verify on Windows that a product whose source
  cannot tell "used up" apart from "nothing here" never shows 「已用满」, and that
  four or more enabled products scroll instead of shrinking below 240px.
- PARITY-UI-01: official provider artwork (six products, transparent container
  marks) instead of the previous mismatched replacements, and the settings order
  数据同步 → 订阅配额 → 常规 → 数据目录（高级）→ 测试诊断 → 关于 → 危险操作 with the
  set-once directory controls collapsed and ZCode's key form inside its own
  row. Capture cropped screenshots of the panel and the settings window on
  Windows; browser rendering on macOS is not native evidence.

## Commands and evidence

Use Windows x64, Node 22 (node:sqlite), pnpm >=10.8 <11, Rust 1.88 and MSVC/SDK. Keep the
same deliberately long clone location, with CLI in sibling `vibe-usage`.
Do not import old target/dist/node_modules. Do not re-vendor npm latest.

In the CLI checkout (with the original PS7 host module-path contamination):

```powershell
node --test
```

In the Windows checkout, save every command's full output and native exit code:

```powershell
node scripts/check-version.mjs
powershell -NoProfile -File scripts/test-windows-build-paths.ps1
# Also run with pwsh if already installed:
pwsh -NoProfile -File scripts/test-windows-build-paths.ps1
pnpm install --frozen-lockfile
pnpm test
pnpm build
node scripts/fetch-node.mjs
powershell -NoProfile -File scripts/cargo-windows.ps1 test --workspace
powershell -NoProfile -File scripts/cargo-windows.ps1 test --workspace --features external-test-diagnostics
powershell -NoProfile -File scripts/cargo-windows.ps1 test -p vibe-usage-app --features external-test-diagnostics process_lifecycle -- --nocapture
node src-tauri/resources/cli-bootstrap.mjs --version   # 启动器能解析并运行 npm latest
powershell -NoProfile -File scripts/cargo-windows.ps1 test -p vibe-usage-app --features external-test-diagnostics credential_manager_roundtrip_isolated -- --ignored --nocapture
pnpm run release:windows:test
```

Native lifecycle tests must execute: shutdown gate, child/grandchild cleanup
without killing an unrelated helper, cancellation, and bundled-node image
unlock/removal after shutdown. Do not skip these to obtain green results.
The Node removal fixture copies only the bundled runtime to a temporary dir,
launches a no-network dummy script, and removes only its own copy.
CLI should still run 377 cases with 10 explained Windows skips; compare names
and reasons. ACL setup/cleanup failures remain failures.

Test the Cargo entry with unset and explicit short CARGO_TARGET_DIR, verify
that it returns a deliberately failing Cargo command's nonzero code, and that
its caller environment is restored. Do not claim bare Cargo gained a hook.
Run release from the isolated pnpm entry used in the failed baseline, without
manually adding its shim directory to PATH; require the wrapper to recover it.
Both automatic and explicit target release runs remain required.

## Quit/uninstall regression

Produce VibeUsage-0.5.12-Windows-External-Test-Setup.exe; record SHA256, bytes,
Authenticode and diagnostic identity. appBuild is windows-acceptance-<Unix secs>.
The installed executable must run from the installation directory.

1. Exercise UI quit and tray quit while sync and quota work are in flight.
   Observe owned app/node processes, quit latency and exit timestamps. Ensure
   unrelated terminals/Node processes remain alive. Do not globally taskkill node.
2. Immediately start silent uninstall after quit, without manually waiting for
   all child processes to disappear (that would bypass the reported race).
   Run at least five cycles, preserving the first failure and every residual
   file/directory listing. Test normal idle quit as a separate control.
3. Verify the three-second NSIS retry and any reboot-required outcome honestly.
   Do not reboot the user's machine automatically or silently remove leftover
   files to turn a failure into a pass. Record cleanup separately.
4. Verify same-version reinstall/uninstall, final absence of installation entries
   and processes, shared config hash preservation and test settings restoration.
   Do not reset shared config or overwrite existing ZCode credentials.
5. Recheck quota selection/persistence, real Codex/Kimi, external update isolation,
   windows/tray, sync, dates and filters. For date/filter correctness, capture
   selected values and settled results before/after; opening a menu is not proof
   of a changed result. Active's shared cumulative-session semantics stay unchanged.

The existing provider subscriptions may be reused; no purchases. no_data is not
positive quota success. Claude/Grok/ZCode stay BLOCKED without valid conditions.
Network/sleep disruption needs a user-chosen safe window. Old-version upgrades,
long running and reset boundaries stay NOT RUN until actually exercised.
A valid Windows signature is required before public release; no replacement
certificate or security-setting changes are authorized.

## Return package

Return evidence/REPORT.md, every referenced raw log (including failures),  cropped
and redacted screenshots, installed-external-diagnostics-final.jsonl, the exact
installer (or accessible location), and SHA256SUMS covering every attachment and
installer. No desktop apps, key fragments or raw account/session data in images.

Small Windows fixes may be appended to this branch, with a patch/bundle relative
to the supplied snapshot HEAD and a rebuilt installer with new identity/hash.
No generated artifacts/credentials in Git, no push/PR/npm/Release publication.
Do not mark full acceptance passed: report each remaining gap and prerequisite.

## Cursor 单独登录（2026-11-26 起，PR「Cursor 单独登录」）

前提：npm 上的 CLI 已是 0.15.10 或更新（带 vibe-cafe/vibe-usage#210）；要用真实 Cursor 账号，用谁的账号、什么时候测由维护者当次决定。外测版和正式版用同一条凭据（和 ZCode Key 一样），同一个 Windows 用户下在外测版里「退出」也会删掉正式版的单独登录；要保留正式版的登录，就换一个 Windows 用户测。测完在设置里「退出」，并在 cursor.com 设置的 Sessions 里撤销测试时多出的「Desktop App」条目。截图里不能出现邮箱以外的账号信息，凭据管理器的内容不截图。

1. 设置 →「订阅配额」→ Cursor 开关下面：未登录时左边「未登录」、右边「登录」，没有说明文字。把 npm 的 CLI 换成旧版不可行，所以「CLI 不支持就不显示按钮」只由前端单元测试覆盖。
2. 点「登录」：默认浏览器打开 `https://cursor.com/loginDeepControl?…`（页面标题「Sign in to Cursor CLI」）；这一行变成「请在浏览器里确认；如果 Cursor 让你去邮箱确认，点完邮件里的链接再回来」+「取消」。确认（需要时去邮箱点链接）后 5 分钟内变成邮箱 +「退出」，随后自动同步一次。
3. 再点一次「登录」后点「取消」：这一行回到「登录」，任务管理器里没有残留的、属于 Vibe Usage 的 `node.exe`。
4. 控制面板 → 凭据管理器 → Windows 凭据 → 普通凭据：只多出 `ai.vibecafe.vibe-usage.windows/cursor/login`（凭据长时还有 `/2`），没有动别的条目。Windows 安全中心（Defender）全程没有提示。
5. 本机 Cursor 已加密或没登录的情况下，单独登录后 Cursor 额度卡片能读到 `Month`；本机 Cursor 已加密且没有单独登录时，卡片显示「需要登录 Cursor」，点整张卡片打开设置并滚到 Cursor 这一行（短暂高亮）。
6. 日志：登录、同步、刷新额度之后，在 `%LOCALAPPDATA%\ai.vibecafe.vibe-usage.windows\logs\sync.log` 里搜 `accessToken`、`refreshToken`、`eyJ`（令牌开头），都搜不到。「复制诊断信息」的内容里也没有。
7. 代理：开着 Clash 的系统代理（再测一次 PAC 模式），Clash 的连接列表里能看到 `cursor.com`、`api2.cursor.sh` 走代理；把系统代理指到一个没人监听的端口再点「登录」，显示「连不上 Cursor，请检查网络和代理后重试」或「无法按你的代理设置连接 Cursor，未发送任何请求」，没有直连（关掉代理软件后防火墙或路由器日志里没有这两个主机的直连）。
8. 撤销：在 cursor.com 撤销这条「Desktop App」后最多 10 分钟，下一次读取时卡片显示「Cursor 登录已失效」，设置这一行显示「登录已失效」+「重新登录」；之后 Clash 里不再出现发往 `cursor.com`、`api2.cursor.sh` 的新连接。
9. 「退出」：先在同一行展开「退出 Cursor 登录？」「只删除 Vibe Usage 里的这份登录，Cursor 编辑器不受影响。」，点「退出」后凭据管理器里的条目消失，这一行回到「登录」，Cursor 编辑器仍是登录状态。
10. 重启 App、开机自启（系统设了代理时）后，已登录状态还在，额度和同步照常；开机自启、代理还没就绪时，续期和其他读取一样先等代理。
11. 续期要到期前 7 天才会触发，真机上只能等，不改系统时间（会影响别的程序）；由 CLI 实测（11-25 维护者账号强制续期两次）和单元测试覆盖。
12. 卸载后凭据管理器里的条目还在（和 ZCode Key 一样，`PRIVACY.md` 写明了），手动删除即可。
13. 开着「开机自启动」重启电脑：登录后不弹主窗口，只出现托盘图标；点托盘图标，主窗口按上次的大小打开；直接双击桌面快捷方式启动时，主窗口照常弹出。
