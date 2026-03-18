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
2. 再按目标安装

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

### 2. 按目标安装

Claude Code：

```bash
humanize install --target claude
```

Codex：

```bash
humanize install --target codex
```

Kimi：

```bash
humanize install --target kimi
```

全部安装：

```bash
humanize install --target all
```

常见选项：

```bash
# 指定 Claude 安装根目录
humanize install --target claude --plugin-root /custom/path

# 指定 skill 安装目录
humanize install --target codex --skills-dir /custom/skills

# 预览，不落盘
humanize install --target all --dry-run
```

默认位置：

- Claude：Windows 下 `%APPDATA%\\humanize-rs`，macOS 下 `~/Library/Application Support/humanize-rs`，Linux/Unix 下 `${XDG_DATA_HOME:-~/.local/share}/humanize-rs`
- Codex：`${CODEX_HOME:-~/.codex}/skills/`
- Kimi：`~/.config/agents/skills/`

各目标的安装内容：

- `claude`：`.claude-plugin/`、`hooks/`、`commands/`、`agents/`、`docs/images/`
- `codex`：仅 skill 定义
- `kimi`：仅 skill 定义
- `all`：以上全部

`humanize install` 不会安装 binary 本身。
它默认假定 `humanize` 已经在 `PATH` 上。

### Claude Marketplace 安装

通过 Claude marketplace 安装是一个**两步过程**：

1. 先把 `humanize` binary 安装到 `PATH`
2. 再安装 Claude 插件包

例如：

```bash
cargo install humanize-cli --bin humanize
claude plugin marketplace add ./
claude plugin install humanize-rs@humania
```

验证：

```bash
which humanize
claude plugin list
```

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
