# Gemini App 上游

`gemini-app-*` 模型支持两种互斥的接入方式：由本机 App2API 管理授权，或在 DeepGeminiProxy 内直接授权。它们与反重力账号、项目 ID、配额和模型路由隔离，不会相互回退。

## 本机 App2API（不需要自备 Google OAuth 配置）

1. 按 [App2API 的启动文档](https://github.com/ShawTim/gemini-app2api/tree/f41036a47e7b58b6929a3f2901aa806189f1da81#2-start-the-gateway) 在同一台机器上运行 App2API，保持仅监听本机回环地址。打开 `http://127.0.0.1:18989/admin/`，在 **Accounts → Google sign-in** 完成登录，并用账号卡片的 **Test** 验证模型可用性。
2. 在 App2API 的 **API keys → Create key** 创建客户端密钥。把它作为 `ABV_GEMINI_APP_API_KEY` 注入 **DeepGeminiProxy 进程环境**，完全重启应用；不需要复制任何 Google 客户端配置、密码或令牌。默认 API 地址是 `http://127.0.0.1:18989/v1`；如果 App2API 改了端口，用 `ABV_GEMINI_APP_BASE_URL` 覆盖。地址只接受回环 IP（例如 `127.0.0.1` 或 `[::1]`），不走系统代理，也不跟随重定向。示例（在仓库根目录启动 macOS 版开发环境，输入不会回显）：

   ```sh
   printf 'App2API client key: '
   IFS= read -rs ABV_GEMINI_APP_API_KEY
   printf '\n'
   export ABV_GEMINI_APP_API_KEY
   npm run tauri dev
   unset ABV_GEMINI_APP_API_KEY
   ```

3. 客户端继续连接 **DeepGeminiProxy 原来的地址和 API Key**，请求 `gemini-app-neon`、`gemini-app-hydrogen` 或 `gemini-app-flash-lite`。若启用模型白名单，先放行相应名称。网关会将其映射为 App2API 对应的 `gemini-*` 名称，原样转发 JSON 和 SSE（响应可以包含 App2API 的模型名）。支持 `POST /v1/chat/completions` 和 `POST /v1/responses`；其他参数和模型权益以 App2API 的契约为准。

设置网关变量后，Gemini App 标签页会提示去 App2API 管理账号，而不是再发起 Google 授权。「已配置」不代表网关在线或模型已验证。若遇到 `gemini_app_gateway_unreachable`，检查服务和端口；遇到 `gemini_app_gateway_auth_failed`，检查 App2API 的客户端密钥（不是管理员密码或 DeepGeminiProxy 自己的密钥）。要切回直接授权，移除两个网关变量并完全重启；原有本地账号不会被改动。

## 直接授权（不运行 App2API）

1. 和 App2API 一样，**带默认 OAuth 配置的构建可以直接登录**，普通用户不需要设置环境变量。发布者须有权分发已获 Google 授权访问 Gemini App 的桌面 OAuth 配置。普通 Google OAuth 客户端、反重力 OAuth 客户端或 Gemini API Key 不能替代它。源码仓库不提供第三方客户端配置；未提供默认值的构建仍需由部署者配置。

   | 构建时环境变量（发布者提供） | 运行时环境变量（部署者覆盖） | 含义 |
   | --- | --- | --- |
   | `ABV_GEMINI_APP_DEFAULT_OAUTH_CLIENT_ID` | `ABV_GEMINI_APP_OAUTH_CLIENT_ID` | 获授权的 Google 桌面 OAuth 客户端 ID，需允许本机回调和离线访问。 |
   | `ABV_GEMINI_APP_DEFAULT_OAUTH_SCOPE` | `ABV_GEMINI_APP_OAUTH_SCOPE` | Google 向该客户端开放的 Gemini App 授权范围，多个值用空格隔开。应用另请求 `openid email` 校验账号。 |
   | `ABV_GEMINI_APP_DEFAULT_OAUTH_PUBLIC_CLIENT_SECRET` | `ABV_GEMINI_APP_OAUTH_CLIENT_SECRET` | 仅在客户端要求时设置。构建时的值会出现在二进制中，只能用于明确允许公开分发的桌面客户端参数，不能放入保密凭据。 |

   启动时优先读取完整的运行时配置；只有未设覆盖时才使用构建内默认值。两者不混用，填写不完整会报配置无效。不要把保密凭据写进命令历史或提交到 Git。本模式不要设置 `ABV_GEMINI_APP_BASE_URL` 或 `ABV_GEMINI_APP_API_KEY`。
2. 启动或完整重启 DeepGeminiProxy，打开 **新建账号 → Gemini App → 使用 Google 登录**。桌面版打开系统浏览器；Web 管理页需在网关所在机器上打开，再点 **打开 Google 登录页**，因为 Google 回调只能到达该机器的回环监听器。授权完成后，账号自动出现在此 Tab 和「账号管理」列表；可用顶部的 Gemini App 筛选只看这个池。仪表盘的「当前账号」与「最佳账号推荐」都有 App 标签，展示授权状态，不伪造配额排名或固定当前账号。模型权限尚未验证。点击移除只删本地账号；如需撤销授权，请在 Google 账号中操作。
3. 开启代理后，客户端沿用 DeepGeminiProxy 原来的 Base URL 和 API Key。选择 `gemini-app-neon`、`gemini-app-hydrogen` 或 `gemini-app-flash-lite`；若启用了模型白名单，先放行所需模型。只要有可用 Gemini App 账号，无反重力账号也能启动桌面代理。

## 直接授权模式的边界

- **账号隔离。** OAuth 使用 PKCE S256 和一次性回调状态。Refresh token 保存在数据目录独立的 `gemini-app-accounts.json`（Unix `0600`），access token 只缓存于内存。更换 OAuth 客户端 ID 后原授权不再参与调度；`invalid_grant` 会将对应账号标记为需要重新登录。账号不会进入反重力账号列表、项目解析或配额刷新。
- **协议范围。** 当前支持文本 HTTP `POST /v1/chat/completions`、`POST /v1/responses`（及 `/responses`），含 JSON 和 SSE。发送完整历史，不支持工具调用、图片、原生搜索、结构化输出、服务端历史、Responses WebSocket、Anthropic 或 Gemini 原生入口。不支持的请求参数会被拒绝。响应带 `X-Upstream-Provider: gemini-app`。
- **错误与完成。** 无可用 App 授权返回 `503 gemini_app_no_accounts`；账号或上游限流返回 `429`；Google 拒绝权限返回 `403 gemini_app_upstream_denied`。不会自动接受 Gemini 服务条款、替换模型或重放生成。SSE 开始后若上游中断，保留前缀并明确标为不完整；成功需要上游模型证据、业务完成信号和 gRPC 成功状态。
- **服务权限。** Robin 不是公开 Gemini 开发者 API。本地测试能验证协议和隔离，不证明 Google 会授予客户端访问权；只在获授权和适用条款允许的情况下启用。
