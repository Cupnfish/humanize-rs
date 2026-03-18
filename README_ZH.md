# Humanize

Humanize 的 Rust 版本仓库说明。

English version: [README.md](./README.md)

## 项目来源

本仓库是对原始 Humanize 项目的 Rust 重写：

- 原项目：<https://github.com/humania-org/humanize/tree/main>

共享插件包名称是 `humanize-rs`。

## 概览

Humanize 提供三类核心能力：

- `RLCR`：实现循环 + Codex review
- `PR loop`：PR review bot 跟踪与验证
- `ask-codex`：一次性 Codex 咨询

## 工作流示意

RLCR 工作流：

![RLCR Workflow](docs/images/rlcr-workflow.svg)

运行时状态保存在项目目录下的 `.humanize/`：

- `.humanize/rlcr/`
- `.humanize/pr-loop/`
- `.humanize/skill/`

## 架构

现在的 Humanize 只保留两类交付物，再加一个外部 review backend：

1. `humanize` binary
   Rust 运行时引擎。内嵌提示词模板，负责循环、hook、校验、monitor 和 Codex 调度。
2. 插件包
   仓库根目录的 `.claude-plugin/`、`hooks/`、`commands/`、`agents/`、`skills/`、`docs/images/`。
   这同一套包同时给 Claude Code 和 Droid 使用。
3. Codex CLI
   RLCR、PR 校验和 `ask-codex` 使用的独立 reviewer backend。

不再保留 Codex/Kimi 作为宿主的单独安装路径。
Codex 现在只保留 reviewer 角色。

## 仓库结构

- `crates/core`：状态、文件、git、codex、模板等核心逻辑
- `crates/cli`：`humanize` 可执行文件
- `prompt-template/`：嵌入到 binary 的提示词模板源文件
- `skills/`：Claude Code / Droid 插件内附带的 `SKILL.md`
- `hooks/`：插件 hook 配置
- `commands/`：插件 slash command 定义
- `agents/`：辅助 agent 定义
- `.claude-plugin/`：共享插件元数据
- `docs/`：安装与使用文档

## 安装方式

推荐顺序：

1. 先把 `humanize` 安装到 `PATH`
2. 再把 `codex` 安装到 `PATH`
3. 最后在 Claude Code 或 Droid 中安装插件包

### 1. 安装 `humanize`

从 crates.io 安装：

```bash
cargo install humanize-cli --bin humanize
```

从当前仓库安装：

```bash
cargo install --path crates/cli --bin humanize
```

或手动把 release binary 放到 `PATH`：

```bash
cargo build --release
cp target/release/humanize /usr/local/bin/humanize
```

验证：

```bash
which humanize
humanize --help
```

### 2. 安装 Codex CLI

Humanize 使用 Codex 作为独立 reviewer backend。
单独安装 Codex CLI，并确保 `codex` 在 `PATH` 上。

验证：

```bash
codex --version
```

### 3. 安装插件包

同一套插件包同时支持 Claude Code 和 Droid。
Droid 官方文档说明它可直接兼容 Claude Code 插件，这个仓库也已经在本地通过 `droid plugin install` 验证。

Claude Code：

```bash
claude plugin marketplace add https://github.com/Cupnfish/humanize-rs.git
claude plugin install humanize-rs@humania-rs
```

Droid：

```bash
droid plugin marketplace add https://github.com/Cupnfish/humanize-rs.git
droid plugin install humanize-rs@humanize-rs
```

插件包包含：

- `.claude-plugin/`
- `hooks/`
- `commands/`
- `agents/`
- `skills/`

`humanize` 可执行文件仍然来自 `PATH`。
binary 不负责安装插件资产，也不再单独安装 skill。

运行时 binary 已经内嵌提示词模板。
仓库顶层的 `prompt-template/` 是提示词源文件，`skills/` 是插件包内附带的 skill 源文件。

## 如何使用插件

安装到 Claude Code 或 Droid 之后，用户的主要入口应该是宿主里的插件命令和 skill，而不是直接调用底层 CLI。
插件会在后台调用 `humanize`。

不同宿主的命令命名不一样：

- Claude Code：插件命令和可直接调用的 skill 带 `humanize-rs:` 前缀
- Droid：相同命令不带 `humanize-rs:` 前缀

### 快速开始

Claude Code：

```bash
/humanize-rs:gen-plan --input draft.md --output docs/plan.md
/humanize-rs:start-rlcr-loop docs/plan.md
/humanize-rs:resume-rlcr-loop
/humanize-rs:start-pr-loop --claude
/humanize-rs:resume-pr-loop
/humanize-rs:cancel-rlcr-loop
/humanize-rs:ask-codex Explain the latest review result
```

Droid：

```bash
/gen-plan --input draft.md --output docs/plan.md
/start-rlcr-loop docs/plan.md
/resume-rlcr-loop
/start-pr-loop --claude
/resume-pr-loop
/cancel-rlcr-loop
/ask-codex Explain the latest review result
```

两个宿主暴露的是同一套工作流能力：

- 从 draft 生成 plan
- 启动 RLCR loop
- 从 `.humanize/` 恢复现有 RLCR loop
- 启动 PR loop
- 从 `.humanize/` 恢复现有 PR loop
- 取消当前 RLCR 或 PR loop
- 直接咨询 Codex

### 插件会自动处理什么

- hooks 会自动调用 Rust 实现的 validators 和 stop hooks
- RLCR / PR loop 状态会保存在 `.humanize/`
- 插件内附带的 `humanize`、`humanize-rlcr`、`humanize-gen-plan`、`ask-codex` skill 会提供给宿主，并在合适时被自动调用

### RLCR 的典型用户流程

1. Claude Code 中运行 `/humanize-rs:gen-plan --input draft.md --output docs/plan.md`；Droid 中运行 `/gen-plan --input draft.md --output docs/plan.md`
2. Claude Code 中运行 `/humanize-rs:start-rlcr-loop docs/plan.md`；Droid 中运行 `/start-rlcr-loop docs/plan.md`
3. 之后继续像平常一样在 Claude Code 或 Droid 中工作
4. 每次宿主停止输出时，Humanize hooks 会自动校验状态、触发 Codex review，并决定是继续、阻塞还是推进阶段
5. 如果你想在终端里实时观察状态，可以额外打开 monitor

如果宿主 session 丢失，但 `.humanize/rlcr/` 还在，不要重新开 loop，直接恢复：

- Claude Code：`/humanize-rs:resume-rlcr-loop`
- Droid：`/resume-rlcr-loop`

### 什么时候直接用 CLI

直接调用 `humanize` CLI 主要用于：

- monitor 面板
- 调试
- 手动恢复
- 没有 hook 的环境

例如：

```bash
humanize gen-plan --input draft.md --output docs/plan.md
humanize setup rlcr docs/plan.md
humanize resume rlcr
humanize gate rlcr
humanize resume pr
humanize ask-codex "Explain the latest review result"
```

### Monitor

快照模式：

```bash
humanize monitor rlcr --once
```

TUI 模式：

```bash
humanize monitor rlcr
```

示例监控界面：

![Humanize Monitor TUI](docs/images/monitor-tui.svg)

### 手动恢复 / Hook 调试

直接手动触发 stop：

```bash
printf '{}' | humanize stop rlcr
printf '{}' | humanize stop pr
```

在 skill-mode 或没有 hook 的环境里手动执行 gate：

```bash
humanize gate rlcr
```

Gate 返回码：

- `0`：允许继续
- `10`：被阻塞，需要按提示处理
- `20`：运行时或基础设施错误

## 提示词和 Skill 在哪里

- 提示词模板：`prompt-template/`
- skill 源文件：`skills/`

改完模板或插件资产后：

- 重新构建并安装 `humanize` binary
- 在 Claude Code 或 Droid 中重新加载或重新安装插件

## 其他文档

- [docs/usage.md](./docs/usage.md)
- [docs/install-for-claude.md](./docs/install-for-claude.md)
- [docs/install-for-droid.md](./docs/install-for-droid.md)
- [docs/install-for-codex.md](./docs/install-for-codex.md)
