# DeepGeminiProxy 发布状态

原有的 Windows 单独打包、GitHub Release 和 GitHub Pages Actions 已停用。当前 [Verify](../.github/workflows/verify.yml) 只做验证；推送提交或标签不会由此工作流上传安装包、网站或更新清单。

- 日常 Git 交付按 [git-commit](../.agents/skills/git-commit/SKILL.md) 处理，不顺手升版本、打标或发布。
- 需要发版时先明确目标分支、版本、平台、签名及产物去向，再对候选提交按 [TEST.md](../TEST.md) 验证。新发布流程单独设计，不沿用旧仓库的发布凭据、镜像源或自动更新配置。
- 现有安装链接、已发布的 Releases/Pages 和客户端更新源不会因删除 Actions 自动迁移或撤下。
