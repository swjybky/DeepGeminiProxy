# Windows Agent 安装检测

**症状**　Windows 已安装 DeepWrite、DeepSeek Harness 或 Codex 桌面版，Agent 接入页显示「未检测到安装」。

**根因**　DeepWrite 旧版在 Windows 直接返回未安装。DSH 和 Codex 的旧检测主要依赖 CLI 命令及少量固定路径，遗漏了图形安装器和 Microsoft Store 登记。

**处理**　使用含修复的 DeepGeminiProxy 重新打开 Agent 接入页。DeepWrite 和 DSH 的 Windows 安装器会登记卸载信息，Codex 的商店版和独立 MSIX 安装会登记当前用户的软件包。DeepWrite 在 Windows 可显示安装状态，一键接入仅支持 macOS。从浏览器打开接入页时，DeepWrite 会显示「当前环境无法检测安装」。

**预防**　在 Windows 分别验证默认目录、自定义目录和卸载后的检测结果。用只装桌面版且 CLI 不在 PATH 的测试账号验证 DSH 和 Codex，确认安装状态与接入能力分别呈现。
