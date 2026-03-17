# 迁移计划：将 Humanize 项目从 Bash/Python 迁移到 Rust

## 1. 概述

Humanize 是一个 Claude Code 插件，提供迭代开发循环（RLCR）和 PR 循环功能。当前实现主要由 Bash 脚本、Python 辅助脚本、Markdown 命令定义文件和 JSON 配置组成。目标是将其核心逻辑迁移到 Rust（edition = "2024"），同时保留与 Claude Code 插件系统的必要接口。

**迁移原则**：
- 逐步替换：从最核心、最独立的模块开始，逐步迁移，确保每个阶段功能可用且测试通过。
- 保持接口兼容：现有 Claude Code 插件系统（命令文件、钩子脚本）应继续工作，其内部实现指向 Rust 二进制。
- 最大复用：将通用逻辑（状态管理、文件操作、Git/Codex 交互、模板渲染）提取到 Rust 库中，多个 CLI 命令共享。
- 测试先行：将现有 Bash 测试集转换为 Rust 集成测试，确保行为一致。

## 2. 项目结构设计

新建 Rust 工作区（workspace），包含多个 crate：

```
humanize/
├── Cargo.toml                  # 工作区定义
├── crates/
│   ├── humanize-core/           # 核心库：状态、文件、Git、Codex、模板等
│   │   └── Cargo.toml
│   ├── humanize-cli/            # CLI 入口：处理 slash 命令（start-rlcr-loop, cancel, ask-codex 等）
│   │   └── Cargo.toml
│   ├── humanize-hooks/          # 钩子处理：接收 JSON 输入，调用 core 逻辑，返回 JSON 决策
│   │   └── Cargo.toml
│   ├── humanize-monitor/        # 终端监控程序（原 humanize monitor）
│   │   └── Cargo.toml
│   ├── humanize-gen-plan/       # 生成计划的专用工具（原 validate-gen-plan-io.sh 及 AI 交互）
│   │   └── Cargo.toml
│   └── humanize-pr/             # PR 循环专用工具（原 fetch-pr-comments, poll-pr-reviews 等）
│       └── Cargo.toml
├── scripts/                     # 保留少量胶水脚本（过渡），最终可能移除
├── tests/                       # 集成测试（重写原 Bash 测试）
├── commands/                    # Markdown 命令文件（保留，但 allowed-tools 指向 Rust 二进制）
├── hooks/                       # 钩子脚本（保留，内部调用 Rust 钩子二进制）
├── prompt-template/              # 模板文件（保留，Rust 代码读取）
└── .claude-plugin/               # 插件元数据（保留）
```

**说明**：
- `humanize-core` 提供所有非 CLI 功能：状态解析、Git 操作、Codex 调用、模板渲染等。
- `humanize-cli` 解析 `/humanize:xxx` 命令参数，调用 core 执行，并输出结果（或调用子命令）。
- `humanize-hooks` 是单独的可执行文件，被钩子脚本调用，接收 JSON 输入，输出 JSON 决策（block/allow）。每个钩子类型（PreToolUse, Stop 等）可共享同一个二进制，通过子命令区分。
- `humanize-monitor` 实现 `humanize monitor rlcr|pr|skill` 功能，使用终端控制。
- `humanize-gen-plan` 和 `humanize-pr` 作为独立工具，可被 `humanize-cli` 调用或直接执行。

## 3. 核心模块功能分解

### 3.1 状态管理 (humanize-core/src/state.rs)
- 解析 `state.md` 和 `finalize-state.md` 中的 YAML frontmatter。
- 定义 `State` 结构体，包含所有字段（`current_round`, `max_iterations`, `plan_file`, `session_id`, `agent_teams` 等）。
- 实现 `parse` 和 `save` 方法（保存时写入 YAML）。
- 提供辅助函数：`find_active_loop()`，`resolve_state_file()` 等。
- 处理会话 ID 过滤逻辑。

### 3.2 文件系统操作 (humanize-core/src/fs.rs)
- 安全路径操作（禁止符号链接、路径遍历检查）。
- 读写 `.humanize/` 目录下的各种文件。
- 复制、移动、删除文件（如重命名 state.md 为 complete-state.md）。
- 备份 plan 文件。

### 3.3 Git 交互 (humanize-core/src/git.rs)
- 封装 git 命令调用，处理超时（使用 `std::process::Command` + timeout）。
- 获取当前分支、检查工作区是否干净、获取提交 SHA、检查是否为祖先（用于 force-push 检测）、检测 rebase/merge 状态。
- 集成 `humanize_parse_git_status` 逻辑（统计修改/添加/删除/未跟踪文件）。
- 提供 `is_ancestor`、`get_ahead_count` 等函数。

### 3.4 Codex 交互 (humanize-core/src/codex.rs)
- 调用 `codex exec` 和 `codex review` 命令，处理超时和环境变量。
- 解析 review 输出，检测 `[P0-9]` 模式。
- 封装 `ask_codex` 功能（单次咨询）。
- 处理 `HUMANIZE_CODEX_BYPASS_SANDBOX` 环境变量（危险标志）。

### 3.5 模板渲染 (humanize-core/src/template.rs)
- 使用轻量模板引擎（如 `tera` 或自定义 `{{VAR}}` 替换）。
- 从 `prompt-template/` 加载模板文件，支持变量替换。
- 提供安全回退（模板缺失时返回默认消息）。

### 3.6 钩子逻辑 (humanize-core/src/hooks.rs)
- 实现各个钩子的具体验证逻辑（如 `loop-bash-validator` 中的 `command_modifies_file` 检查）。
- 这些逻辑可被 `humanize-hooks` 二进制调用，也可被其他部分复用。

### 3.7 循环核心 (humanize-core/src/rlcr.rs, pr.rs)
- **RLCR 循环**：`stop_hook` 的核心逻辑，包括阶段判断、Codex 调用、状态更新、prompt 生成等。
- **PR 循环**：`pr-loop-stop-hook` 的核心逻辑，包括触发检测、bot 超时、更新 `active_bots`、处理 +1 反应等。
- 这些模块应独立于 CLI，接收必要的输入（state、环境）并返回决策/输出。

### 3.8 监控 (humanize-monitor)
- 使用 `crossterm` 或 `ratatui` 实现实时终端监控。
- 轮询 `.humanize/rlcr` 和缓存目录，解析状态文件，显示进度。
- 处理终端大小变化、SIGINT 等。

### 3.9 计划生成 (humanize-gen-plan)
- 调用 `validate-gen-plan-io` 功能（Rust 实现）。
- 与 AI 代理交互（通过 Task 工具？可能仍需调用外部命令，或直接调用 Claude API？当前使用 Task 工具调用 agent，可能需要保持相同机制，即生成 prompt 并让 Claude 执行。Rust 部分主要负责 IO 验证、模板组合、调用外部命令）。
- 可复用 `humanize-core` 中的模板和文件操作。

## 4. 替换策略

### 4.1 脚本替换顺序

1. **核心库 `humanize-core`**：首先编写，包含所有不依赖外部命令的纯逻辑。同时编写 Rust 测试覆盖。
2. **钩子二进制 `humanize-hooks`**：逐个钩子替换。每个钩子脚本改为调用该二进制：
   ```bash
   #!/bin/bash
   exec /path/to/humanize-hooks <hook-name> --input "$(cat)"
   ```
   确保输出 JSON 格式与原脚本一致。
3. **CLI 命令 `humanize-cli`**：替换所有 `/humanize:xxx` 命令的实现。Markdown 命令文件中的 `allowed-tools` 修改为指向新二进制。
4. **监控 `humanize-monitor`**：替换 `humanize monitor` 功能。
5. **PR 循环工具**：逐步替换 `fetch-pr-comments.sh`, `poll-pr-reviews.sh` 等为 Rust 二进制。
6. **其他工具**：`ask-codex.sh`, `cancel-*.sh` 等。

### 4.2 胶水代码

- 初期保留 Bash 脚本，但内容简化为调用 Rust 二进制。
- 最终所有 Bash 脚本都可移除，仅保留少量用于环境检测或安装的脚本（也可用 Rust 替代）。
- 模板文件、Markdown 命令文件、插件元数据 JSON 保持不变，因为它们不包含可执行代码。

### 4.3 兼容性注意事项

- 环境变量：Rust 程序应读取相同的环境变量（如 `CLAUDE_PROJECT_DIR`、`HUMANIZE_CODEX_BYPASS_SANDBOX`）。
- 退出码：保持与原脚本一致（例如 0 成功，1 失败，124 超时等）。
- JSON 输入/输出：钩子输入 JSON 格式与原脚本相同；输出 JSON 也需一致。
- 文件路径：Rust 中应使用 `std::path::Path` 处理，避免硬编码。

## 5. 测试策略

- **单元测试**：在 `humanize-core` 中为每个函数编写测试（使用 `#[cfg(test)]`）。
- **集成测试**：在 `tests/` 目录下创建 Rust 集成测试，调用新二进制并验证输出。逐步将现有 Bash 测试转换为 Rust。
- **模拟外部命令**：在测试中模拟 `git`, `codex`, `gh` 等，可使用 `mockall` 或创建临时 mock 脚本并设置 PATH。
- **端到端测试**：保留一些关键场景的手动测试，直到 CI 完善。

## 6. 依赖选择

- **CLI 解析**：`clap`（支持子命令）。
- **错误处理**：`anyhow` + `thiserror`。
- **序列化**：`serde` + `serde_yaml` + `serde_json`。
- **Git 操作**：直接调用 `git` 命令，使用 `std::process::Command`。可考虑 `git2` 但依赖系统 libgit2，可能增加复杂度，暂不使用。
- **模板引擎**：`tera`（功能强大，但较重）或自制简单替换（使用 `regex`）。推荐 `tera`，因为模板有复杂逻辑的可能性。
- **异步**：可能不需要，所有操作可同步完成。监控部分需轮询，可使用 `std::thread::sleep` 和 `crossterm` 事件。
- **终端控制**：`crossterm` 或 `ratatui`（推荐 `ratatui` 构建 TUI）。
- **测试 mock**：`mockall` 或自定义。

## 7. 与 Claude Code 集成

- **命令注册**：Claude Code 插件通过 `commands/` 目录下的 Markdown 文件定义命令。其中的 `allowed-tools` 字段可以指向可执行文件。我们需要将路径指向 `humanize-cli` 二进制。
  例如：
  ```yaml
  allowed-tools:
    - "Bash(${CLAUDE_PLUGIN_ROOT}/target/release/humanize-cli rlcr-start:*)"
  ```
- **钩子注册**：`hooks/hooks.json` 中的 `command` 路径也应指向 `humanize-hooks`。
- **环境变量**：Claude Code 会设置 `CLAUDE_PLUGIN_ROOT`，Rust 程序应使用此变量查找模板等资源。

## 8. 详细迁移步骤

### 阶段 0：准备
- 创建 Rust 工作区，设置 `Cargo.toml`。
- 编写 `humanize-core` 的基础骨架，定义状态结构体，实现部分简单函数（如文件读写）。
- 添加必要的依赖。

### 阶段 1：替换钩子验证器
- 选择最独立的钩子，如 `loop-read-validator.sh`。
- 在 `humanize-core` 中实现其核心逻辑（检查文件是否允许读取）。
- 创建 `humanize-hooks` 二进制，支持子命令 `read-validator`，读取 stdin JSON，调用 core 逻辑，输出结果 JSON。
- 修改原钩子脚本为调用该二进制。
- 运行现有测试确保行为不变。
- 重复此过程替换所有钩子：`write-validator`, `edit-validator`, `bash-validator`, `plan-file-validator`, `codex-stop-hook`, `pr-loop-stop-hook`。

### 阶段 2：替换 CLI 命令
- 选择简单的命令，如 `cancel-rlcr-loop`。
- 在 `humanize-core` 中实现取消逻辑（查找 loop 目录，创建信号文件，重命名）。
- 创建 `humanize-cli` 二进制，支持子命令 `cancel-rlcr-loop`。
- 修改 `commands/cancel-rlcr-loop.md` 中的 `allowed-tools` 指向新二进制。
- 测试。
- 逐步替换所有命令：`start-rlcr-loop`, `gen-plan`, `ask-codex`, `start-pr-loop`, `cancel-pr-loop`。

### 阶段 3：替换监控
- 在 `humanize-monitor` 中实现监控逻辑，使用 `ratatui` 构建 TUI。
- 集成 `humanize-core` 中的状态解析和文件轮询。
- 替换 `scripts/humanize.sh` 中的 `humanize monitor` 函数，改为调用新二进制。

### 阶段 4：替换 PR 工具脚本
- 将 `fetch-pr-comments.sh`, `poll-pr-reviews.sh`, `check-bot-reactions.sh`, `check-pr-reviewer-status.sh` 等脚本用 Rust 重写，集成到 `humanize-pr` crate 或作为子命令。
- 确保它们被 `humanize-hooks` 或 `humanize-cli` 正确调用。

### 阶段 5：优化与清理
- 移除所有不再需要的 Bash 脚本。
- 确保安装脚本（`install-skill.sh` 等）也能调用 Rust 二进制（或重写为 Rust）。
- 完善文档和示例。

## 9. 风险与挑战

- **外部命令依赖**：`git`, `codex`, `gh` 的版本和行为差异。需要在 Rust 中谨慎处理输出解析和错误情况。
- **超时处理**：原 Bash 脚本有超时机制（`run_with_timeout`），Rust 中需使用 `std::process::Command` 配合线程或 `wait_timeout` crate 实现。
- **信号处理**：监控程序需要处理 SIGINT 和 SIGWINCH，确保终端恢复。
- **YAML frontmatter 解析**：state.md 中的 YAML 可能包含注释、多行字符串等，需使用健壮的解析器（`serde_yaml` 支持 YAML 1.2）。
- **兼容性**：确保 Rust 程序生成的 state.md 与旧版本兼容（字段顺序、引号处理）。

## 10. 结论

该迁移计划将 Humanize 的核心逻辑从 Bash/Python 转移到 Rust，提高代码可维护性、性能和安全性。通过逐步替换和严格的测试，可以确保功能一致且平稳过渡。最终，Humanize 将成为一个高效、跨平台的 Claude Code 插件，同时为未来功能扩展打下坚实基础。
