# Humanize

Humanize 的 Rust 版本仓库说明。

English version: [README.md](./README.md)

## 项目来源

本仓库是对原始 Humanize 项目的 Rust 重写：

- 原项目：<https://github.com/humania-org/humanize/tree/main>

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

现在的 Humanize 只保留一个运行时，再加一个外部 review backend：

1. `humanize` binary
   Rust 运行时引擎。内嵌提示词模板，负责循环、hook、校验、monitor 和 Codex 调度。
2. Codex CLI
   RLCR、PR 校验和 `ask-codex` 使用的独立 reviewer backend。

不再保留 Codex/Kimi 作为宿主的单独安装路径。
Codex 现在只保留 reviewer 角色。

## 仓库结构

- `crates/core`：状态、文件、git、codex、模板等核心逻辑
- `crates/cli`：`humanize` 可执行文件
- `prompt-template/`：嵌入到 binary 的提示词模板源文件
- `skills/`：插件包的 `SKILL.md` 源文件
- `hooks/`：插件 hook 配置源文件
- `commands/`：插件 slash command 源文件
- `agents/`：Claude agent 与 Droid droid 的源定义
- `.claude-plugin/`：为兼容性保留的遗留插件元数据
- `docs/`：安装与使用文档

## 安装方式

推荐顺序：

1. 先把 `humanize` 安装到 `PATH`
2. 再把 `codex` 安装到 `PATH`
3. 最后把 Humanize 安装到宿主里

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

### 3. 安装到宿主

Claude Code：

```bash
humanize init --global
```

这会直接调用 Claude Code 自己的 plugin manager：

- 按需添加 Humanize marketplace source
- 在 user scope 安装或更新 `humanize-rs` 插件
- 记录一次与当前 CLI 版本绑定的 sync stamp，用于之后检测 CLI / plugin 是否失配

验证：

```bash
humanize init --global --show
humanize doctor
```

Droid：

```bash
droid --version
humanize init --global --target droid
```

这会直接调用 Droid 自己的 plugin manager：

- 按需添加 Humanize marketplace source
- 在 user scope 安装或更新 `humanize-rs` 插件
- 记录一次与当前 CLI 版本绑定的 sync stamp，用于之后检测 CLI / plugin 是否失配

验证：

```bash
humanize init --global --target droid --show
humanize doctor --target droid
```

`humanize` 可执行文件仍然来自 `PATH`。
主安装方式已经切到 `humanize init`。

运行时 binary 已经内嵌提示词模板。
仓库顶层的 `prompt-template/` 是提示词源文件，`skills/`、`commands/`、`agents/`、`hooks/` 是插件包源文件。

## 版本维护

把根目录 [Cargo.toml](/home/cupnfish/humanize/Cargo.toml) 里的 workspace version 当成唯一真源。

升级版本时执行：

```bash
cargo xtask sync-version
cargo xtask verify-version-sync
```

`sync-version` 会自动更新 `.claude-plugin/` 下的 plugin manifest。

## 如何在宿主里使用 Humanize

安装到 Claude Code 或 Droid 之后，用户的主要入口应该是宿主里的命令和 skill，而不是直接调用底层 CLI。
安装好的宿主插件会在后台调用 `humanize`。

用 `humanize init` 安装后，两个宿主都会暴露相同的 `/humanize-rs:*` 命名空间 slash command。
`ask-codex` 仍然作为 skill 可用。

### 快速开始

Claude Code：

```bash
/humanize-rs:gen-plan --input draft.md --output docs/plan.md
/humanize-rs:start-rlcr-loop docs/plan.md
/humanize-rs:resume-rlcr-loop
/humanize-rs:start-pr-loop --claude
/humanize-rs:resume-pr-loop
/humanize-rs:cancel-rlcr-loop
/ask-codex Explain the latest review result
```

Droid：

```bash
/humanize-rs:gen-plan --input draft.md --output docs/plan.md
/humanize-rs:start-rlcr-loop docs/plan.md
/humanize-rs:resume-rlcr-loop
/humanize-rs:start-pr-loop --claude
/humanize-rs:resume-pr-loop
/humanize-rs:cancel-rlcr-loop
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

### Init 会自动处理什么

- 通过 Claude Code / Droid 原生 plugin CLI 安装或更新宿主插件
- 写入一个把当前 `humanize` CLI 版本和宿主插件绑定起来的 sync stamp
- 之后当 CLI 版本和上次同步版本不一致时，提醒你重新执行 `humanize init`

### Doctor

可以用 `humanize doctor` 检查：

- 当前 CLI 版本
- marketplace 配置
- 宿主插件是否已安装
- sync stamp 状态
- 是否需要重新执行 `humanize init`

### RLCR 的典型用户流程

1. 在 Claude Code 或 Droid 中运行 `/humanize-rs:gen-plan --input draft.md --output docs/plan.md`
2. 运行 `/humanize-rs:start-rlcr-loop docs/plan.md`
3. 之后继续像平常一样在宿主里工作
4. 每次宿主停止输出时，Humanize hooks 会自动校验状态、触发 Codex review，并决定是继续、阻塞还是推进阶段
5. 如果你想在终端里实时观察状态，可以额外打开 monitor

如果宿主 session 丢失，但 `.humanize/rlcr/` 还在，不要重新开 loop，直接恢复：

- `/humanize-rs:resume-rlcr-loop`

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

改完模板或宿主资产源文件后：

- 重新构建并安装 `humanize` binary
- 对目标宿主重新执行 `humanize init --global`

## 其他文档

- [docs/usage.md](./docs/usage.md)
- [docs/install-for-claude.md](./docs/install-for-claude.md)
- [docs/install-for-droid.md](./docs/install-for-droid.md)
- [docs/install-for-codex.md](./docs/install-for-codex.md)
