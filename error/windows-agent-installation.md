# Windows Agent 安装检测

**症状**　Windows 已安装 DeepWrite、DeepSeek Harness 或 Codex 桌面版，Agent 接入页显示「未检测到安装」。旧版代理在 Windows 检测到 DeepWrite 时还会显示「当前平台不支持一键接入」。

**根因**　旧版 DeepWrite 接入只实现了 macOS 钥匙串加密。Windows 使用当前用户的 DPAPI 和 DeepWrite 配置目录中的 Electron 密钥。DSH 和 Codex 的旧检测主要依赖 CLI 命令及少量固定路径，遗漏了图形安装器和 Microsoft Store 登记。

**处理**　使用含修复的 DeepGeminiProxy 桌面版。DeepWrite 一键接入在 macOS 和 Windows 中按模型和密钥字段识别独立密钥文件或合并配置，也支持迁移过的数据目录。字段结构沿用时，配置版本号增加后仍可接入。首次接入前先启动 DeepWrite，完成模型配置及系统安全存储的初始化。旧版 DeepWrite 缺少共享配置锁，接入和断开前需要退出。DeepWrite 和 DSH 的 Windows 安装器会登记卸载信息，Codex 的商店版和独立 MSIX 安装会登记当前用户的软件包。

**预防**　在 Windows 分别验证默认目录、自定义目录和卸载后的检测结果。用只装桌面版且 CLI 不在 PATH 的测试账号验证 DSH 和 Codex，确认安装状态与接入能力分别呈现。
