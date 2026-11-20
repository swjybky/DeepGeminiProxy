# Gemini App2API 反向代理技术报告

**分析对象**：[ShawTim/gemini-app2api](https://github.com/ShawTim/gemini-app2api)，`v1.6.0`，提交 [`f41036a`](https://github.com/ShawTim/gemini-app2api/commit/f41036a47e7b58b6929a3f2901aa806189f1da81)。本报告以该提交的网关源码、契约文档和离线测试用例为依据，对照 DeepGeminiProxy `1298dceb` 的实现。结论来自静态审阅，不代表实测兼容性。

## 1. 结论

Gemini App2API 并不是把 HTTP 请求转发给一个正在运行的 Gemini App。它是一个**双向协议转换网关**：向下游提供 OpenAI 风格的 HTTP/JSON 和 SSE，向上游直接调用 Gemini 消费端的 Robin 服务，自己处理 HTTP/2 上的 gRPC 帧和 Protobuf 报文。主网关不依赖本地 Gemini App 进程或浏览器会话。仓库另外提供 Pi 和 OpenClaw 扩展，本报告只分析 HTTP 网关路径。

对 DeepGeminiProxy，最值得借鉴的是**区分上游已完成、下游已交付、客户端是否应重试**。不建议直接移植 Robin 私有协议，也不建议把 App2API 的 `2,048` 字节首段缓冲或 `400/422` 错误映射照搬到现有客户端：我们的流式路径特意提前发头、用心跳覆盖长 prefill，这与对方延迟发头的目标不同。

## 2. 请求如何穿过网关

```text
OpenAI 客户端
  → Bearer 鉴权 + Chat / Responses 请求归一化
  → 账户选择、会话亲和、请求期限
  → 渲染完整历史；按需准备图片附件
  → Gemini Robin（HTTP/2 + gRPC 帧 + Protobuf）
  ← 累计文本快照、模型证据、结束信号
  ← 快照转增量、工具与终态校验
  ← 按端点交付 JSON 或 SSE
```

| 层 | 实现与边界 |
| --- | --- |
| API 契约 | `GET /v1/models` 列出网关支持的别名与元数据，不代表账号实时权益。`POST /v1/chat/completions` 和 `POST /v1/responses` 归一到共享消息结构。客户端逐轮发送完整历史；网关不静默截断，不支持以 `previous_response_id` 或 `conversation` 代替历史。部分采样、输出 token 上限参数仅做兼容接收，不实际执行；估算用量明确标记 `estimated`，不能用来对账。见 [openai-contract.mjs](https://github.com/ShawTim/gemini-app2api/blob/f41036a47e7b58b6929a3f2901aa806189f1da81/src/openai-contract.mjs)。 |
| 账户调度 | 网关校验下游 API key，选中账号后使用该账号授权的 access token 调上游，两类凭据分开。会话亲和按调用方和会话隔离，只提供路由偏好，不对同会话的不同请求加互斥锁；首选账户忙碌可改选，旧请求晚结束不会覆盖更新请求的路由结果。配置允许的恢复受尝试次数、候选账户和请求期限约束。见 [pool.mjs](https://github.com/ShawTim/gemini-app2api/blob/f41036a47e7b58b6929a3f2901aa806189f1da81/src/gateway/pool.mjs)。 |
| 上游传输 | `openDirectTurn` 负责组帧、HTTP/2 会话、keepalive、取消及接收上限；这一层不负责换号。`NativeDirectProvider` 解码业务帧，将累计快照转为增量，核验请求与实际解析出的模型模式，部分模型还校验归属，避免把降级结果标成请求的模型。正常传输结束也不能替代模型完成信号。见 [native-transport.mjs](https://github.com/ShawTim/gemini-app2api/blob/f41036a47e7b58b6929a3f2901aa806189f1da81/src/gateway/native-transport.mjs) 和 [native-provider.mjs](https://github.com/ShawTim/gemini-app2api/blob/f41036a47e7b58b6929a3f2901aa806189f1da81/src/native-provider.mjs)。 |
| 工具与结构化输出 | 调用方声明的函数通过提示词中的 `tool_call` 约定模拟，不是 Gemini 原生结构化函数调用。网关验证名称、参数 Schema、`tool_choice` 与历史调用/结果 ID，工具由客户端执行；合法参数不意味着执行获授权。要求 JSON 输出且最终没有工具调用时，先缓冲再校验终稿，不能把 `strict` 当成上游约束解码。原生搜索是单独的受限路径，不能与调用方工具混为一谈。见 [API_SUPPORT.md](https://github.com/ShawTim/gemini-app2api/blob/f41036a47e7b58b6929a3f2901aa806189f1da81/docs/API_SUPPORT.md)。 |
| 交付 | `app.mjs` 协调请求取消、账户尝试和 HTTP 状态，`response-delivery.mjs` 负责首段缓冲、序列化和终态收尾。慢客户端写入也有超时；不能因为有首段文本就把整轮记成成功。见 [app.mjs](https://github.com/ShawTim/gemini-app2api/blob/f41036a47e7b58b6929a3f2901aa806189f1da81/src/gateway/app.mjs) 和 [response-delivery.mjs](https://github.com/ShawTim/gemini-app2api/blob/f41036a47e7b58b6929a3f2901aa806189f1da81/src/gateway/response-delivery.mjs)。 |

**请求合并不等于模型续写。** 开启自动恢复且有稳定会话标识时，同一调用方、会话、端点及请求体可共享进行中的生成，由多个 HTTP 客户端订阅；失败或不完整结果不缓存。成功缓存设 `60s` 到期，但正常交付后、最后一个订阅结束时也可提前移除，不能把 `60s` 当作重放保证。它不是跨进程缓存、模型 KV 断点或工具执行幂等机制；重放仍需客户端按调用 ID 去重。见 [request-resumption.mjs](https://github.com/ShawTim/gemini-app2api/blob/f41036a47e7b58b6929a3f2901aa806189f1da81/src/gateway/request-resumption.mjs)。

## 3. 流式设计：何时可以重试，何时只能收尾

普通公开流式请求不会仅因上游返回 `200` 就发下游响应头。前 `2,048` 字节 UTF-8 文本（不是字符数或 token 数）先保留；达到门槛、出现已验证工具调用或正常完成时才放行。正常短答不必填满门槛，等到完成即可发出。延迟提交为短残片重试留出了空间，代价是响应头和首个正文块可能更晚到达。

| 场景 | App2API 的处理 |
| --- | --- |
| 尚未提交，收到同响应 ID 的文本改写 | 可以替换暂存草稿，不把两个版本拼起来；若已提交正文或工具调用，再收到冲突改写，保留已交付内容并标记中断。 |
| 尚未提交，只有短残片，上游提前断开 | 满足恢复条件且有可用尝试时，可丢弃短残片，发起一次 `short_output` 替换；这不是从残片继续生成，也不是所有短答都重试。 |
| 已交付可用文本或有效工具调用，上游中断 | 不再开启新回合拼接。Chat 用 `stop`/`tool_calls`、`incomplete: true` 和 `[DONE]` 收尾；Responses 补齐已交付项的 `done` 事件，再发带 `incomplete: true` 的 `response.completed`。这是为了让客户端保留内容的兼容扩展，**不是上游完整结束的证明**。 |
| 至少尝试过一次、尚未发头、零正文且零工具调用，且失败原因在允许列表内、内部恢复已结束 | Chat 返回 `422 GENERATION_FAILED`，Responses 返回 `400 GENERATION_FAILED`，带 `X-Should-Retry: false`、`client_action: "stop_turn"`，同时保留上游错误码及状态。鉴权、额度、参数错误等不套用这条映射。 |
| 下游连接已断，终态事件无法写入 | 客户端应视为结果未知或中断；不能从已有 HTTP `200` 或收到过正文推断成功。 |

这套契约将**失败责任**与**客户端下一步动作**分开。比如上游账户额度不足应由运营方处理，不意味着 API 客户端应重放同一轮。参见上游 [COMPLETION_CONTRACT.md](https://github.com/ShawTim/gemini-app2api/blob/f41036a47e7b58b6929a3f2901aa806189f1da81/docs/COMPLETION_CONTRACT.md)。

## 4. 与 DeepGeminiProxy 的差异及建议

| 维度 | 本仓库现状（代码对照） | 建议 |
| --- | --- | --- |
| 上游与入口 | [Axum 路由](../../src-tauri/src/proxy/server.rs) 同时服务 OpenAI、Anthropic、Gemini 等协议；所对照的 Gemini 主路径由 [`UpstreamClient`](../../src-tauri/src/proxy/upstream/client.rs) 调 Cloud Code `v1internal`，与 Robin 的报文、模型证据及授权边界不同。 | **不移植私有上游。** 借鉴传输与交付分层；模型实际值只有在现有上游提供可靠证据时才宣称已验证。 |
| 何时发头 | [OpenAI handler](../../src-tauri/src/proxy/handlers/openai.rs) 对流式 Chat 和 Responses 提前返回响应，由 [stream lifecycle](../../src-tauri/src/proxy/pipeline/stream_lifecycle.rs) 发心跳；Chat 非流式路径使用 [stream peek](../../src-tauri/src/proxy/pipeline/stream_peek.rs)。 | **优先保留心跳策略。** 先区分 `headers_committed`（HTTP 状态已不可变）与 `semantic_output_committed`（正文或工具已不可替换），记录头延迟、正文 TTFT 和零输出率。两者不能用一个布尔值互相替代。 |
| 终态与重试 | [terminal](../../src-tauri/src/proxy/pipeline/terminal.rs) 和 [rotation](../../src-tauri/src/proxy/pipeline/rotation.rs) 已有终态分类和账号轮转。对照的 [OpenAI mapper](../../src-tauri/src/proxy/mappers/openai/streaming.rs) 中，Chat 正常收尾路径会在有正文/工具但缺少 `finishReason` 时补 `stop`/`tool_calls`；Responses 对缺少完成证据的收尾可发 `response.incomplete`，且有独立的失败路径。 | **先统一内部交付事实，再做协议映射。** 区分完整、已交付但中断、零输出失败，并记录失败原因与是否仍允许重试；现有终态及会话持久化语义先保持不变。若要变更 `response.completed`、HTTP 状态或重试提示，先用目标客户端验证，尤其不能让部分回合因为事件改名而被存成已验证完成。 |
| 账号、合并与资源 | 本仓库已有 [TokenManager](../../src-tauri/src/proxy/token_manager.rs)、会话与轮换策略。App2API 的软亲和、短期响应共享和无默认冷却，与这些现有策略并非等价。 | **不替换现有配额及冷却机制。** 只有确认断线导致明显重复生成后，再评估跨租户隔离、慢订阅者内存限制和工具去重；先用分离的请求数/上游尝试数度量收益。 |

## 5. 后续验证计划与运行边界

建议先为本仓库补合成上游回归，不接真实账号。优先断言三件事：**每轮上游尝试次数、下游有且只有一个协议终态（可写连接时）、会话只保存本来应该保存的结果。**

| 合成用例 | 观察项 |
| --- | --- |
| 发头前零输出：EOF、reset、timeout；另测鉴权、配额 | 可重试与不可重试原因分开；不输出空成功；不把传输失败标成 token 上限或误伤账户。 |
| 已发心跳但未发正文；已发正文后中断 | 分别记录发头与正文提交；已交付正文不重复、不拼接；不能在 `200` SSE 中伪装成新的 JSON HTTP 错误。 |
| 正常短答；多字节字符跨块；工具 JSON 半截或校验不通过 | 正常短答不丢失、UTF-8 输出不破损、无效工具不交给客户端执行。已发出有效工具调用后不另起替换回合。 |
| Chat、Responses 各覆盖流式与非流式；再用目标客户端连接本地假上游 | 检查发头/首字延迟、终态解读、自动重发次数，以及 Responses 会话存储。确认兼容前不更改公共返回契约。 |

上游已有 [Current Contract 离线清单](https://github.com/ShawTim/gemini-app2api/blob/f41036a47e7b58b6929a3f2901aa806189f1da81/docs/CURRENT_CONTRACT_TESTS.md)，包含零输出、部分交付、同 ID 改写和字节上限用例，可参考断言而不照搬策略。本次只审阅源码和测试用例，未执行外部项目代码、未登录、未调用 Gemini，不对 SDK 表现、延迟、配额或生产稳定性做实测结论。

**部署风险。** 对方将管理 Cookie/Origin/CSRF 和 `/v1/*` Bearer key 分离；远程图片默认关闭，开启后仍需限制目的地址、逐跳校验重定向并设大小/时间上限。未取得明确授权前，不把非公开消费端接口作为生产能力：仓库 [MIT 许可](https://github.com/ShawTim/gemini-app2api/blob/f41036a47e7b58b6929a3f2901aa806189f1da81/LICENSE) 不授予上游服务使用权，项目 [免责声明](https://github.com/ShawTim/gemini-app2api/blob/f41036a47e7b58b6929a3f2901aa806189f1da81/DISCLAIMER.md) 也明确提醒服务条款、限流及账号风险。排错与验证沿用本仓库的 [ERROR.md](../../ERROR.md) 和 [TEST.md](../../TEST.md)，不保留真实凭据或请求正文作为夹具。
