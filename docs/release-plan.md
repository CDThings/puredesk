# PureDesk 发布总纲：GitHub 开源 + 官网分发

> 目标：一条命令发版（推 tag 即自动出安装包），两条下载路径（GitHub Release + 官网/镜像），
> 一个信任锚（签名 + 校验和 + 可复现构建）。

## 0. 一次性准备（发布前做一次）

| # | 事项 | 说明 |
| --- | --- | --- |
| 1 | 仓库卫生 | 确认 `LICENSE`（MIT）、`README`（含截图/教程）、`.gitignore`（不含 `puredesk-local.pfx` 等私钥）、无绝对路径/隐私信息残留 |
| 2 | Actions 权限 | Settings → Actions → General → Workflow permissions = **Read and write permissions**（Release 上传需要） |
| 3 | 仓库 Topics | `windows` `desktop-organizer` `tauri` `rust` `desktop` `file-organizer`（决定搜索曝光） |
| 4 | Issue 模板 | `.github/ISSUE_TEMPLATE/bug.md`（附 puredesk.log 尾部 + 系统版本）与 `feature.md` |

## 1. 发版流程（每版一次，推 tag 即发）

```bash
# 1) 更新版本号（三处同步）：package.json / src-tauri/Cargo.toml / src-tauri/tauri.conf.json
# 2) 更新 README「更新记录」
# 3) 提交 + 打标签
git commit -m "v0.2.3: ..."
git tag v0.2.3 && git push origin main --tags
# 4) GitHub Actions「release」workflow 自动：测试 → 构建 → 出 NSIS 安装包 + 便携 zip + SHA256SUMS → 创建 Release
```

产物清单（Release 资产）：
- `PureDesk_0.2.3_x64-setup.exe` —— NSIS 安装包（主推，双击安装+开始菜单+卸载器）
- `PureDesk_0.2.3_x64_portable.zip` —— 便携版（解压即用，适合免安装场景）
- `SHA256SUMS.txt` —— 校验和（信任锚，官网展示）

## 2. 手动兜底流程（CI 不可用时）

```bash
npm run build                                   # 前端
cd src-tauri && cargo build --release --features custom-protocol
npx tauri build                                 # NSIS 安装包 → target/release/bundle/nsis/
gh release create vX.Y.Z PureDesk_*.exe PureDesk_*_portable.zip SHA256SUMS.txt --generate-notes
```

## 3. SmartScreen / 签名策略（决策项）

| 方案 | 成本 | 效果 |
| --- | --- | --- |
| **A. 不签名（首发推荐）** | 0 | 下载时 Edge/Chrome 提示"保留即可"，SmartScreen 点「更多信息→仍要运行」。README 与官网放醒目指引；应用内已有低完整性自检横幅兜底 |
| B. 自签 + 内置根证书安装器 | 低 | 只对本机有效，无全局意义，不推荐 |
| C. OV 代码签名证书 | ~¥800-2000/年 | 立即无警告；积累声誉后 SmartScreen 自动放行 |
| D. Microsoft Store 上架 | $19 一次性开发者账号 + MSIX 打包 | 完全无警告、可商店更新，未来可选 |

首发用 A + 官网/README 教学截图；用户量起来后再评估 C/D。

## 4. 自动更新（tauri-plugin-updater，v0.3 落地）

1. 生成密钥对：`npx @tauri-apps/cli signer generate -w ~/.tauri/puredesk.key`
2. 私钥 → 仓库 Secrets（`TAURI_SIGNING_PRIVATE_KEY[_PASSWORD]`）；公钥 → `tauri.conf.json → bundle.updater.pubkey`
3. Release workflow 追加 `--config '{"bundle":{"createUpdaterArtifacts":true}}'`，产物自动带 `.sig`
4. `latest.json`（版本号/下载地址/签名）上传到 Release，并在 `https://CDThings.github.io/puredesk/latest.json` 放一份镜像
5. 应用内：启动后静默检查（24h 间隔）+ 设置页「检查更新」按钮；下载失败静默降级，不打扰

> Windows 端 updater 是整包替换（无差量），安装包 ~8MB 可接受。不建议做前端资源"热更新"：
> 破坏签名一致性、引入版本漂移故障面，收益仅省几 MB 下载——性价比为负，不做。

## 5. 官网（下载落地页）

- **形态**：GitHub Pages 单页（`gh-pages` 分支或 `/docs`），域名可选 `puredesk.app` 或先裸 `https://CDThings.github.io/puredesk/`
- **页面元素**：首屏截图 + 特性三栏 + 「下载最新版」大按钮 + SmartScreen 指引 + 更新日志
- **稳定下载直链**（永远指向最新版，不随版本号变化）：
  `https://github.com/CDThings/puredesk/releases/latest/download/PureDesk_x64-setup.exe`
  （NSIS 产物建议不带版本号命名，或在 Release 里固定放一份别名文件）
- **国内加速**（GitHub 直连慢的三层兜底）：
  1. Release 页说明使用镜像代理前缀（如 `https://ghproxy.com/` 类公共实例，注明第三方风险）
  2. Gitee 同步镜像仓库（README 指向，Release 附件需手动同步）
  3. Cloudflare Pages/自建 CDN 托管 latest.json 与安装包（后续流量大时）

## 6. 上架分发渠道（放大触达）

| 渠道 | 动作 | 优先级 |
| --- | --- | --- |
| **winget** | 提交 manifest 到 `microsoft/winget-pkgs`，之后用户可 `winget install PureDesk`；每次发版 PR 更新版本 | 高（免费信任背书） |
| awesome-tauri / awesome-windows-apps | 提 PR 收录 | 中 |
| Scoop bucket | 自建 bucket 或提交 main bucket | 中 |
| 少数派/吾爱破解等社区 | 发介绍帖（自备截图与更新日志） | 发布稳定后 |

## 7. 发布后运营

- README 徽章：release version、CI status、license、downloads（shields.io）
- 每个版本 Release Notes 用 CI 自动生成（`generate_release_notes: true`）
- issue 里引导附上 `puredesk.log`（应用内「打开日志」入口可后续加）
- 数据指标：Release 下载量 / star 趋势 / issue 响应时长

## 8. 发布检查清单（每版执行）

- [ ] 三处版本号一致
- [ ] README 更新记录已写
- [ ] `cargo test` 19+ 全绿、`tsc --noEmit` 干净
- [ ] 新 exe 真机冒烟（启动链路/整理/撤销/还原/托盘）
- [ ] 卸载路径验证：安装包卸载后 `桌面整理` 文件夹与文件完好（只删程序不删数据）
- [ ] SHA256SUMS 已生成并上传
- [ ] 官网 latest.json / 下载直链已指向新版
