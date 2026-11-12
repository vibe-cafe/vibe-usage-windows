# Windows 发版说明（Release notes）

## 发版说明从哪来

`.github/workflows/release.yml` 用 `softprops/action-gh-release` 创建 Release 时不传 `body`，所以 GitHub Release 页面上显示的发版说明，就是 `v*` 标签自带的说明文字（带注释标签的 message）。也就是说，发版说明是打标签时人工写的，流水线里没有模板。轻量标签（`git tag v1.2.3` 不带 `-a`）没有说明文字，Release 页面会是空的。

## 模板

打标签时用 `git tag -a vX.Y.Z -F <说明文件>`，说明文件按下面的格式写：

```text
Vibe Usage for Windows X.Y.Z

- 修复：……（#PR）
- 功能：……（#PR）

首次安装如果看到「Windows 已保护你的电脑」，点「更多信息」→「仍要运行」。

签名说明：[Code signing policy](https://github.com/vibe-cafe/vibe-usage-windows#code-signing-policy)
```

## 检查项

- [ ] 标签是带注释的标签，第一行是 `Vibe Usage for Windows X.Y.Z`。
- [ ] 每一条改动写清是修复还是功能，并带上 PR 编号。
- [ ] 倒数第二段是安装提示。安装包换成 Windows 信任的签名、并且下载后不再被 SmartScreen 拦下之前，每一版都要保留这一句。
- [ ] 最后一行是指向 README「Code signing policy」一节的链接。SignPath Foundation 的条款要求「Code signing policy」这个词出现在项目主页和下载/发布页上，Release 页面就是我们的发布页。
- [ ] 推送标签后，打开 Release 页面，确认说明文字显示完整，并且能看到 Code signing policy 链接、点开能跳到 README 的那一节。
