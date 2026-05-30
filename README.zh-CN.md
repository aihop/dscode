# dscode

> 移动优先的 AI 编码代理，基于 DeepSeek。

**dscode** 是一个终端原生的 AI 编码代理，构建在 codewhale 引擎之上。
它直接连接 DeepSeek 的 API，完全通过命令行工作——
零网页界面、零 TUI、零臃肿。专为 **手机 SSH** 和 **Android Termux** 优化。

```bash
# 一行安装
curl -fsSL https://dscode.org/install.sh | sh

# 设置密钥，开始对话
dscode auth login
dscode chat
```

---

## 特性

### 代理智能
- **30+ 工具** — 文件 I/O、shell、git（读写）、代码搜索、网络搜索、URL 获取、补丁应用、代码审查、FIM 编辑、子代理、checklist、测试运行器、记忆。
- **7 种子代理角色** — `explore`（只读调研）、`plan`（设计+checklist）、`architect`（高层设计）、`coder`（实现）、`reviewer`（审查）、`tester`（写测试）、`verifier`（运行验证）。每种角色有定制系统提示和工具权限——只读角色不能写文件。
- **Plan 模式** — 输入 `/mode plan` 切换到只读调研模式，写代码前先调研。`/mode agent` 切回全工具模式。
- **自动验证** — 每次编辑后自动运行语法检查：`.rs` 跑 `cargo check`、`.py` 跑 `python -m py_compile`、`.js` 跑 `node --check`、`.ts` 跑 `tsc --noEmit`、`.go` 跑 `go vet`、`.c/.cpp` 跑 `gcc/g++ -fsyntax-only`。结果标记 `[VERIFY PASS]` 或 `[VERIFY FAIL]`。
- **跨会话记忆** — 用 `remember(key, value)` 告诉模型你的偏好。它会持久化到 `memory.md`，每次新对话自动注入。用 `recall(query)` 查询。
- **JSON 修复** — 移动网络中断导致的流式工具调用参数截断，自动补全缺失的引号、花括号和方括号。

### 移动优先
- **纯 CLI** — 无 TUI、无网页 UI，只有 stdin/stdout。SSH 零延迟。
- **窄终端适配** — 自动检测 ≤80 列终端，自动换行，精简输出。
- **零系统依赖** — 只需要 `git`。不需要 `grep`、`curl` 等系统命令。内置搜索使用 `ignore` crate（Rust 原生、识别 .gitignore）。
- **API 韧性** — 自动重试 + 指数退避（3 次，500ms/1s），应对不稳定移动网络。
- **单二进制** — ~3.4MB，静态链接，ARM 就绪。

### 工程能力
- **流式 Markdown** — 完整 Markdown → ANSI 渲染（标题、粗体、代码块带语法高亮、列表、表格、引用）。
- **会话持久化** — 基于 SQLite，支持保存/恢复/列表/导出对话。
- **模型回退** — `deepseek-v4-pro` 失败时自动回退到 `deepseek-v4-flash`。
- **推理力度** — 默认 `medium` 级别思考。`--think low/high` 覆盖。
- **命令安全** — 策略引擎拦截破坏性命令（`rm -rf /`、`dd`、`mkfs` 等）。
- **审批模式** — `--approve` 在写文件或运行 shell 前提示确认。

## 快速开始

### 1. 安装

```bash
curl -fsSL https://dscode.org/install.sh | sh
```

或者从源码构建：

```bash
git clone --recursive https://github.com/aihop/dscode.git
cd dscode
cargo build --release -p dscode
cp target/release/dscode ~/.local/bin/
```

### 2. 认证

```bash
dscode auth login
# 输入你的 DeepSeek API key：sk-...
```

或者设置环境变量：

```bash
export DEEPSEEK_API_KEY=sk-your-key-here
```

### 3. 对话

```bash
# 交互模式（默认）——代理模式，带工具
dscode chat

# 单次提问
dscode run "写一个 Rust 版的 fibonacci 函数"

# 使用 Flash 模型，响应更快
dscode chat -m deepseek-v4-flash

# 以 Plan 模式启动（只读调研）
dscode chat --plain

# 开启审批模式
dscode chat --approve
```

### 4. 行内命令

```text
/mode plan      切换到只读调研模式
/mode agent     切换回全工具模式
/clear          清屏
/save           立即保存会话
/exit           退出
```

## 命令列表

| 命令 | 说明 |
| ---- | ---- |
| `dscode chat` | 交互式对话（代理模式） |
| `dscode chat --plain` | 不带工具的对话 |
| `dscode chat -s <id>` | 恢复指定会话 |
| `dscode run <prompt>` | 单次提问，打印回复 |
| `dscode auth login` | 设置 API key（隐藏输入） |
| `dscode auth test` | 验证 API key 有效性 |
| `dscode auth status` | 查看认证状态 |
| `dscode config init` | 交互式配置向导 |
| `dscode config show` | 查看当前配置 |
| `dscode session list` | 列出保存的会话 |
| `dscode session show <id>` | 查看会话详情 |
| `dscode session rename <id> <name>` | 重命名会话 |
| `dscode session delete <id>` | 删除会话 |
| `dscode session export <id>` | 导出会话为 JSON |
| `dscode tools list` | 列出所有可用工具 |
| `dscode model` | 列出可用模型 |
| `dscode completion bash` | 生成 shell 补全 |

## 代理工具

dscode 内置 30+ 个工具。随时用 `dscode tools list` 查看。

```
read_file           读取文件内容
write_file          创建或覆盖文件
edit_file           精确文本替换
apply_patch         应用 unified-diff 补丁
run_shell           执行 shell 命令（拦截破坏性命令）
search_code         用正则搜索项目文件
search_symbols      查找定义（函数、类、结构体、trait）
file_search         模糊文件名搜索
web_search          网络搜索（DuckDuckGo）
fetch_url           HTTP GET 获取 URL
list_files          列出目录内容
list_tree           树形目录结构
get_file_info       文件元数据 + 预览
git_log             提交历史
git_show            提交详情 + diff
git_blame           每行最后修改者
git_status          工作目录状态
git_diff            工作目录 diff
git_add             暂存文件
git_commit          创建提交
git_push            推送到远程
review              代码审查（文件、diff 或暂存区）
fim_edit            通过 DeepSeek FIM API 做中间填充编辑
agent_open          以指定角色启动子代理
agent_eval          查看子代理状态
agent_close         关闭子代理
remember            持久化存储偏好或规则
recall              查询已存储的记忆
checklist_write     创建任务 checklist
checklist_add       添加 checklist 项
checklist_update    更新项状态
checklist_list      列出所有 checklist 项
test_runner         运行测试并报告结果
request_user_input  在任务中向用户提问
```

## 子代理角色

启动子代理时指定角色，让任务更精准：

```
agent_open(prompt="梳理模块结构", role="explore")
  → 只读调研，返回 path:line 证据

agent_open(prompt="设计迁移方案", role="plan")
  → 设计方案 + checklist，不实现

agent_open(prompt="实现解析器", role="coder")
  → 全工具权限，写代码

agent_open(prompt="审查 diff", role="reviewer")
  → 只读审查，按严重程度打分

agent_open(prompt="跑测试并报告", role="verifier")
  → 只运行验证，不修复
```

## 会话管理

会话自动保存到 `~/.local/share/dscode/state.db`（SQLite）。

```bash
# 列出所有会话（最近优先）
dscode session list

# 查看会话详情
dscode session show abc12345

# 恢复会话
dscode chat -s abc12345

# 重命名方便识别
dscode session rename abc12345 "my-fix-branch"

# 导出为 JSON
dscode session export abc12345 > backup.json

# 删除
dscode session delete abc12345
```

## 移动端使用

### Android (Termux)

```bash
pkg install curl git
curl -fsSL https://dscode.org/install.sh | sh
dscode auth login
dscode chat
```

### 通过 SSH

```bash
ssh user@your-server
dscode chat
```

CLI 自动适应终端宽度——在窄屏手机上无需横向滚动。网络问题自动重试。

## 配置

配置文件路径：`~/.dscode/config.toml`：

```toml
api_key = "sk-..."

[providers.deepseek]
model = "deepseek-v4-pro"
base_url = "https://api.deepseek.com/beta"
```

记忆文件：`~/.dscode/memory.md`

## 架构

```
┌──────────────────────────────────────────────────────┐
│                  dscode CLI (~5,200 行)               │
│                                                       │
│  chat.rs ──agent loop──► api.rs ──SSE──► DeepSeek    │
│    │  ▲                      │                       │
│    │  │                      ▼                       │
│    │  │              engine.rs (自动检查             │
│    │  │               + 验证门禁)                    │
│    │  │                      │                       │
│    │  └── tools/ ────────────┘                       │
│    │      file | git | search | agent                │
│    │                                                 │
│    └── session.rs ◄── state.db ──► codewhale-state   │
│                                                       │
│  外部依赖：仅 git                                     │
│  Rust crates：codewhale-tools, codewhale-config,     │
│               codewhale-state, codewhale-execpolicy, │
│               codewhale-protocol, codewhale-agent     │
└──────────────────────────────────────────────────────┘
```

dscode 构建在 **codewhale 引擎** 之上 —— 复用其工具框架、配置系统、SQLite 存储和命令安全策略，同时保持轻量、移动优先的 CLI 界面。

## 链接

- 仓库：https://github.com/aihop/dscode
- API：https://api.deepseek.com/beta
- 许可证：MIT
