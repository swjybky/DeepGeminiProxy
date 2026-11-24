# Documentation index

This folder contains developer-focused documentation (architecture, implementation details, and validation steps).

## Development workflow

Start with [AGENTS.md](../AGENTS.md). See [DESIGN.md](../DESIGN.md) for UI, [TEST.md](../TEST.md) for validation, [ERROR.md](../ERROR.md) for troubleshooting, and the [release guide](RELEASE_GUIDE.md) for current publishing status.

## Proxy
- [冷启动模型档位未聚合](../error/model-catalog-cold-start.md) — 模型目录、分组缓存与路由排查。
- [Argon 共享配额](../error/argon-shared-quota.md) — 排查官方可选但代理目录缺项。
- [`docs/proxy/auth.md`](proxy/auth.md) — proxy authorization modes, expected client behavior, and implementation pointers.
- [`docs/proxy/accounts.md`](proxy/accounts.md) — account lifecycle in the proxy pool (including auto-disable on `invalid_grant`) and UI behavior.
- [网关残留已删除账号](../error/proxy-deleted-account.md) — 排查配额刷新写回、账号池重载和会话绑定。
- [Gemini App 上游](proxy/gemini-app.md) — 本机 App2API、直接 OAuth、模型选择与部署边界。
- [Gemini App2API 反向代理技术报告](proxy/gemini-app2api-analysis.md) — 协议转换、流式交付边界及对 DeepGeminiProxy 的建议。

## z.ai (GLM) integration
- [`docs/zai/implementation.md`](zai/implementation.md) — end-to-end “what’s implemented” and how to validate it.
- [`docs/zai/mcp.md`](zai/mcp.md) — MCP endpoints exposed by the proxy (Search / Reader / Vision) and upstream behavior.
- [`docs/zai/provider.md`](zai/provider.md) — Anthropic-compatible passthrough provider details and dispatch modes.
- [`docs/zai/vision-mcp.md`](zai/vision-mcp.md) — built-in Vision MCP server protocol and tool implementations.
- [`docs/zai/notes.md`](zai/notes.md) — research notes, constraints, and future follow-ups (budget/usage, additional endpoints).

## Agent Integrations
- [Windows Agent 安装检测](../error/windows-agent-installation.md) — DeepWrite、DeepSeek Harness 和 Codex 桌面版的检测入口。
- [DeepWrite API Key 解密失败](../error/deepwrite-key-decryption.md) — macOS 接入后的密钥排查和重新同步。
- [`docs/jeikcode_integration.md`](jeikcode_integration.md) — JeikCode integration guide, one-click synchronization, configuration options, and KV-cache optimization.
