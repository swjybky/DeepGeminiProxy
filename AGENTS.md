# DeepGeminiProxy 开发约定

本文件是开发入口。设计、验证、排错和交付的细节按需读取；只保留仍适用的结论和做法。

## 开发阶段

1. **理解：**先看相关代码和 `docs/` 中的设计，写出可观察的成功标准。简单改动直接做；影响公共协议、持久化或大量用户路径的改动先对齐方案、兼容和回退。
2. **预防：**按 [ERROR.md](ERROR.md) 的改动区域预读；涉及界面时读 [DESIGN.md](DESIGN.md)，样式改动在动手前截基线。
3. **实现：**只改当前问题，沿用现有分层，不顺手重排无关代码或补做功能。
4. **验证：**按 [TEST.md](TEST.md) 从定向检查开始，按影响面加测。样式改动须对比前后截图并向用户展示；没有验证的写明原因。
5. **排错：**按 [ERROR.md](ERROR.md) 查症状，先最小复现再修根因；重复或耗时的问题沉淀为条目。
6. **交付：**用户要求提交、推送、PR 或合并时，用 [git-commit](.agents/skills/git-commit/SKILL.md)。提交代码不等于发版；发布状态见 [发版指南](docs/RELEASE_GUIDE.md)。

## 启动环境

在仓库根目录运行 `npm ci`。只调试网页用 `npm run dev`；验证原生窗口、Tauri 命令、系统托盘等桌面能力用 `npm run tauri dev`。不默认启动第二个实例或终止用户正在用的进程。

## 项目边界

| 位置 | 职责 |
| --- | --- |
| `src/pages/`、`src/components/` | 页面与共享 UI；界面文案走 `src/locales/` |
| `src/services/`、`src/utils/request.ts`、`src/types/` | 前端业务调用、桌面/网页请求桥接与类型 |
| `src-tauri/src/commands/`、`src-tauri/src/modules/` | Tauri 命令和账户、配置、进程等业务能力 |
| `src-tauri/src/proxy/handlers/`、`src-tauri/src/proxy/mappers/` | 接入协议、参数和报文转换 |
| `src-tauri/src/proxy/pipeline/` | 与接入协议无关的 Gemini 请求处理、预算、上下文与流生命周期 |

- **先走共享流水线。**OpenAI Responses、OpenAI Chat Completions、Anthropic Claude、Google Gemini 最终归一为 Antigravity 所用的 Gemini 格式。共有问题优先修流水线；仅在共享实现无法保留协议语义时才改协议适配器，并补兼容验证。
- **桌面、Web 与无头模式保持必要能力一致。**新增设置时判断是否应由配置字段、环境变量或 CLI 暴露；修改 Tauri 命令或 Web 路由时核对请求名、入参、返回和错误形态。
- **精准修改。**只格式化本次改动的文件；注释写代码本身说不清的约束。替换有副作用的路径时先校验后执行，给失败保留明确的回退路径。
- **不带敏感数据。**测试、日志、截图和 Git diff 不得出现可用凭据、真实账户信息、私有配置或真实请求正文。使用合成数据与 `example.test`；发现泄露及时移除并处理轮换。
