# 内置 Gemini App 上游验收记录

目标：在新建账号的 Gemini App Tab 中独立授权、管理账号；`gemini-app-*` 走进程内 Robin 上游，普通反重力模型保持原有路径。

环境：macOS arm64；OAuth 使用合成回调与私有临时目录，gRPC 使用本机随机回环端口和合成 HTTP/2 帧。没有登录真实账号，也没有调用 Google。

| 验证 | 结果 |
| --- | --- |
| `cargo test --manifest-path src-tauri/Cargo.toml --locked --lib proxy::upstream::gemini_app` | 9 项通过：OAuth 状态与 PKCE、独立存储和重授权、模型分流、请求限制、gRPC 帧与 trailer、流式终态。 |
| `cargo test --manifest-path src-tauri/Cargo.toml --locked --lib proxy::common::model_gate::tests` | 14 项通过。 |
| `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`、`npm run build`、`git diff --check` | 通过。 |
| 已运行的 Tauri 开发版 HTTP 冒烟 | `/health` 返回 `200`；匿名访问 `/api/accounts/gemini-app` 返回 `401`。 |

默认 OAuth 配置回归：同一 Gemini App 模块重跑后 12 项通过。合成配置覆盖无运行时配置时的默认值、完整覆盖、残缺覆盖不混用，以及实际本机监听器接收拒绝回调后不写账号文件。`cargo fmt --check` 和 `git diff --check` 通过。

本机 App2API 模式：定向 Rust 测试 15 项通过。用随机回环端口的合成网关验证 Chat Completions 与 Responses 的模型映射、独立网关 key、SSE 首段在终态前交付、401 映射、地址限制，以及切换模式后不读取或改写旧授权。前端 `npm run build` 通过。

`v1.0.1` 发布候选：在隔离数据目录启动 macOS 无头构建。`/health` 返回 `200`，匿名账号请求返回 `401`，鉴权后账号列表为空、网关状态未配置，未配置 OAuth 时发起登录返回 `503 oauth_profile_required` 且不写账号文件。`variant_mapping` 和 `official_alignment_tests` 分别仍有 6 项和 4 项失败；在 `v1.0.0` 上复现了同样的失败。

内置 App2API 模式（macOS arm64）：`npm run app2api:prepare` 校验固定源码并准备运行时。`cargo test --manifest-path src-tauri/Cargo.toml --locked --lib embedded_runtime_starts_and_owns_the_login_flow -- --ignored` 通过：实际启动随包引擎，验证单实例文件锁、禁止独立后台、空账号请求返回 503、可发起和取消本机 OAuth 流程、退出后可重新获取锁。`npm run tauri build -- --debug --ci --bundles app --config '{"bundle":{"createUpdaterArtifacts":false}}' -- --locked` 成功生成 `DeepGeminiProxy.app`；确认包内 Node.js 可运行且两份许可证齐全。15 项定向 Rust 回归、格式检查和 `git diff --check` 通过。

条款确认与诊断（macOS arm64）：15 项 Rust 定向测试和 2 项内置运行时测试通过。运行时集成测试验证上游消息脱敏、缺少确认和账号授权失效时拒绝且不写文件；用临时合成账号和 stub 验证只有经认证且明确确认的提交才更新条款状态。前端构建、macOS `.app` 重打包、格式检查和 `git diff --check` 通过。

未覆盖：未登录真实 Google 账号、提交真实条款确认或调用真实模型；未验证 Windows/Linux/Docker 产物、已鉴权的 Web/Tauri 控件、视觉或目标 SDK。启用方式见[上游接入说明](../proxy/gemini-app.md)。
