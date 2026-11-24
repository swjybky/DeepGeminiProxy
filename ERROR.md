# DeepGeminiProxy 排错索引

改动前按区域预读；报错时先留现场，再查相应条目。不把外部服务故障或工作区已有错误归咎于本次改动。

## 按改动区域预读

| 区域 | 先读 |
| --- | --- |
| Tauri 调用、Web 请求、鉴权 | [桌面/Web 请求差异](error/desktop-web-request.md)、[代理鉴权](docs/proxy/auth.md) |
| 账号授权与状态 | [账号生命周期](docs/proxy/accounts.md)、[invalid_grant](docs/proxy-invalid-grant.md) |
| 代理失败和日志 | [503 排查](docs/test_503_issue.md)、[代理监控与日志](docs/proxy-monitor-technical.md) |
| 提交、多人或多会话共用工作区 | [混合工作区](error/mixed-worktree.md) |

## 处理步骤

1. 记录脱敏报错、触发命令或操作、平台、版本，以及错误码或 HTTP 状态。不要记录令牌、私有路径或真实请求正文。
2. 做最小复现；能补回归测试时先让它失败。修根因，不用重试、提高上限、关闭鉴权或放宽校验掩盖问题。
3. 重跑定向用例并检查相关失败路径。两轮仍无进展、缺平台或受外部服务限制时，说明已知事实、未验证项和下一步。
4. 同类错误再次出现或排查耗时，补 `error/英文短名.md`，用“症状 / 根因 / 处理 / 预防”四节，并在两个索引中登记。

## 按症状查找

| 症状 | 条目 |
| --- | --- |
| 网页请求失败但桌面成功，或 Tauri 无法调用命令 | [桌面/Web 请求差异](error/desktop-web-request.md) |
| 401 / 403、代理密钥未被接受 | [代理鉴权](docs/proxy/auth.md) |
| DeepWrite 接入后显示「密钥已配置」但解密失败 | [DeepWrite 密钥排查](error/deepwrite-key-decryption.md) |
| Windows 已安装 Agent 桌面客户端，接入页显示「未检测到安装」 | [Windows Agent 安装检测](error/windows-agent-installation.md) |
| `invalid_grant`、账号授权失效 | [invalid_grant](docs/proxy-invalid-grant.md) |
| 删除反重力账号后，新请求仍使用旧账号 | [网关残留已删除账号](error/proxy-deleted-account.md) |
| 503、无法区分本地代理错误和上游错误 | [503 排查](docs/test_503_issue.md) |
| 冷启动仍显示同模型的多个思考档位 | [冷启动模型列表](error/model-catalog-cold-start.md) |
| 暂存区有他人内容、测试报无关文件错误 | [混合工作区](error/mixed-worktree.md) |
