# DeepWrite 接入后 API Key 解密失败

**症状：**DeepWrite 的自定义模型显示「密钥已配置」，但 DeepGemini 分组测试连接时报「模型 API Key 解密失败」。错误发生在发送网关请求之前，不是网关返回的 401。

**根因：**旧版同步将 macOS 应用显示名 `DeepWrite` 当作 Electron 的安全存储名称。Electron 优先取应用 `package.json` 的 `productName`，没有则取 `name`；例如 DeepWrite 1.8.5 的包名是 `@deepwrite/desktop`。如果钥匙串碰巧还有以显示名命名的旧记录，旧同步也会显示成功，但生成的密文无法解密。共享模型配置的开发版和安装版都能读到这份旧密文。

**处理：**使用含修复的 DeepGeminiProxy，在「Agent 工具」关闭再开启 DeepWrite 接入，重新写入该分组的密钥，再到 DeepWrite 测试连接。不必更换网关密钥或删除钥匙串记录。如果该分组下的模型原本是默认模型，重新选为默认。急需时，也可先在 DeepWrite 编辑单个模型，重新填入当前代理网关的 API Key 并保存。

**预防：**同步按安装包里的 Electron 应用名称定位钥匙串，不回退到显示名；读取不到正确记录时不写模型配置。用合成包清单、测试密钥和临时模型配置验证名称解析、Electron 解密、重新同步覆盖旧密文，以及保留手动配置。
