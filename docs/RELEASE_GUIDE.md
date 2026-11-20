# DeepGeminiProxy 打包与发布

[Verify](../.github/workflows/verify.yml) 只验证代码。[Package and Release](../.github/workflows/package-release.yml) 独立打包 macOS（Apple Silicon / Intel，`.dmg`）、Windows x64（`.exe`，正式版另有 `.msi`）和 Linux x64 / arm64（`.AppImage`、`.deb`、`.rpm`）。

## 只打包，不发布

- GitHub：工作流推送到默认分支后，在 **Actions → Package and Release → Run workflow** 选择要打包的分支，保持 `publish_release` 未勾选。完成后从该运行的 **Artifacts** 下载。
- 本机 macOS：安装 Node.js 22+（与构建目标同架构）、Rust、Xcode 命令行工具和 CMake 后，在仓库根目录运行：

  ```bash
  npm ci
  npm run tauri build -- --ci --bundles dmg --config '{"bundle":{"createUpdaterArtifacts":false}}'
  ```

  安装包位于 `src-tauri/target/release/bundle/dmg/`，架构与本机一致。构建阶段下载并校验固定版本的 App2API，并随安装包分发 Node.js 运行时和第三方许可；用户不需要安装 Node.js。不要用不同架构的 Node.js 进程为该安装包准备资源。

## 发布 GitHub Release

1. DeepGeminiProxy 的独立版本从 `1.0.0` 开始；首次发版直接使用此版本，后续用 `npm run bump <version>` 递增（如 `1.0.1`）。补充对应版本的中英文更新日志，按 [TEST.md](../TEST.md) 验证候选提交，再将包含发布工作流的提交推送到 `origin/main`（正式版）或 `origin/beta`（预发布）。
2. 在同一提交上创建与 `package.json` 版本严格一致的**新**标签，首次发布执行 `git tag v1.0.0` 和 `git push origin v1.0.0`。预发布标签示例：`v1.0.1-beta.1`。也可在对应分支手动运行工作流并勾选 `publish_release`；没有标签时，工作流会为该提交创建版本标签。若标签已指向其他提交，应升版本，不要移动旧标签。
3. 所有构建和附件上传成功后，草稿才会转为公开 Release，包含安装包和 `SHA256SUMS.txt`。正式版标为 Latest，预发布不会更新 Latest。已发布的同名 Release 不会被覆盖；若上传失败，确认同名草稿未发布后，可删除该失败草稿再重跑。

工作流使用生成的 `GITHUB_TOKEN`，无需配置旧仓库凭据。它不配置安装包的发布签名或公证、不生成 `updater.json`，也不推送 Docker 镜像。客户端通过 DeepGeminiProxy 的 GitHub Releases 检查更新；新版本需手动下载安装。
