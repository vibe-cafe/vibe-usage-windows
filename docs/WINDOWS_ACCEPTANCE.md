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
