---
name: git-commit
description: 在 DeepGeminiProxy 提交、推送、创建 Pull Request 或合并时使用；验证本次改动并核对提交身份、暂存范围和远端 SHA。
---

# Git 提交与交付

只在用户明确要求时执行 Git 交付。提交、推送、创建 PR、合并和发布是不同的操作；不擅自打标签、发版或部署。

## 提交前

1. 检查 `git status --short --branch`、`git diff --cached` 和 `git worktree list`。保留已有改动、暂存区和 stash。只暂存本次任务审阅过的路径，不用 `git add .` 或 `git add -A`；若已暂存的无关内容无法安全分离，先停下来说明。
2. 按 [TEST.md](../../../TEST.md) 做 L1；跨层改动按影响面补 L2。只改文档或技能时检查格式、相对链接和 diff；改 Actions 另用 `actionlint` 校验。相同内容已经验证时不重复跑；全量留给 CI。必要检查失败时先弄清归属，不把未通过说成通过。
3. 使用仓库提交身份 `carswj <wenjia@wenjiadeMacBook-Air.local>`。用 `git config --local --get user.name` 和 `git config --local --get user.email` 核对；缺失或不符时只修正本仓库配置，不改全局。用 `git var GIT_AUTHOR_IDENT` 确认生效。
4. 审阅暂存 diff 和 `git diff --cached --check`，确认没有密钥、私有请求、无关改动或额外产物。用简短提交信息说明实际目的；确有 Issue/PR 时才关联。除非用户明确要求，不添加 `Co-authored-by`、`Signed-off-by` 等尾注，也不改已有署名。

## 推送与 PR

- 提交后复查提交文件和工作区。推送前确认 `git remote get-url --push origin` 指向 `swjybky/DeepGeminiProxy`，不向 `upstream` 推送；实际查询目标分支并检查待推送提交，只做快进或按请求新建分支，不强推。远端已前进时停下来说明，不自动合入。
- 对 `origin` 显式指定目标分支；推送后用 `git ls-remote origin refs/heads/<分支>` 核对 SHA。已有无关分支或标签不随本次推送。
- 创建 PR 或合并前核对目标分支、变更范围和检查状态，按 [PR 模板](../../../.github/PULL_REQUEST_TEMPLATE.md) 说明验证和未覆盖项。交付时报告提交 SHA、目标、验证结果和未提交的工作。
