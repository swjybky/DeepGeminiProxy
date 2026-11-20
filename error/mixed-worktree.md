# 混合工作区

**症状：**工作区已有修改或暂存内容；检查失败指向本次未改文件；拟推送范围混入无关提交。

**根因：**工作区、索引或分支历史不全属于当前任务，或者相同文件混有多项改动。

**处理：**先看 `git status --short --branch`、`git diff --cached`、`git worktree list`。只暂存本次任务的明确路径；同一文件的改动难以分离，或已有暂存内容会被夹带，就停下说明。需要区分已有失败时，在独立检出中验证目标版本；不 stash、不 reset 他人工作。

**预防：**提交前核对暂存 diff；推送前核对 `<远端分支>..HEAD`，按 [git-commit](../.agents/skills/git-commit/SKILL.md) 交付。
