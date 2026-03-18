# Humanize

Humanize 的 Rust 版本仓库说明。

English version: [README.md](./README.md)

## 项目来源

本仓库是对原始 Humanize 项目的 Rust 重写：

- 原项目：<https://github.com/humania-org/humanize/tree/main>

Claude Code 插件打包名称现在是 `humanize-rs`。

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

## 仓库结构

- `crates/core`：状态、文件、git、codex、模板等核心逻辑
- `crates/cli`：`humanize` 可执行文件
- `prompt-template/`：运行时提示词模板
- `skills/`：源 `SKILL.md`
- `hooks/`：原生 hook 配置
- `commands/`：命令定义
- `agents/`：辅助 agent 定义
- `docs/`：安装与使用文档

## 安装方式

推荐顺序：

1. 先把 `humanize` 安装到 `PATH`
2. 再安装运行时 assets
3. 需要的话再安装 skills

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

### 2. 安装 Claude 集成文件

```bash
humanize install
```

`claude` 目标的默认探测顺序：

1. 如果传了 `--plugin-root <path>`，优先使用它
2. 否则如果设置了 `CLAUDE_PLUGIN_ROOT`，使用该路径
3. 否则使用平台默认的全局运行时目录

默认全局运行时目录：

- Windows: `%APPDATA%\\humanize-rs`
- macOS: `~/Library/Application Support/humanize-rs`
- Linux/Unix: `${XDG_DATA_HOME:-~/.local/share}/humanize-rs`

如果你想强制安装到指定目录：

```bash
humanize install --plugin-root "$PWD"
```

对于 `--target claude`，安装器会写入：

- `hooks/`
- `commands/`
- `agents/`
- `.claude-plugin/`
- `docs/images/`

不会复制 binary，binary 必须已经在 `PATH` 上。

### 3. 安装 skills

Codex:

```bash
humanize install --target codex
```

Kimi:

```bash
humanize install --target kimi
```

如果当前机器上还没有把 `humanize` 装到 `PATH`，本地开发时也可以临时用 `cargo run -- ...` 代替。

对于 `--target codex` 和 `--target kimi`，安装器只写入 skill 目录。
安装后的 skill 默认假定 `humanize` 已经在 `PATH` 上。

运行时二进制已经内嵌提示词模板。
仓库顶层的 `prompt-template/` 和 `skills/` 现在是开发和维护时的源文件目录。

## 常用命令

### 生成计划

```bash
humanize gen-plan --input draft.md --output docs/plan.md
```

### 启动 RLCR

```bash
humanize setup rlcr docs/plan.md
```

### RLCR Gate

```bash
humanize gate rlcr
```

### 启动 PR Loop

```bash
humanize setup pr --claude
humanize setup pr --codex
```

### Ask Codex

```bash
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

## 提示词和 Skill 在哪里

- 提示词模板：`prompt-template/`
- skill 源文件：`skills/`

改完模板或 skill 后，重新执行 `install --target ...` 即可。

## 其他文档

- [docs/usage.md](./docs/usage.md)
- [docs/install-for-claude.md](./docs/install-for-claude.md)
- [docs/install-for-codex.md](./docs/install-for-codex.md)
- [docs/install-for-kimi.md](./docs/install-for-kimi.md)
