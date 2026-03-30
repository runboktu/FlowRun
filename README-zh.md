中文 | [English](./README.md)

# flow-run

专为 AI Agent 设计的声明式工作流引擎。通过 YAML 定义工作流，支持 HTTP 请求、Shell 命令、条件分支、循环执行、子工作流组合、人工审批等功能。

## 安装

```bash
cargo build --release
# 二进制文件位于 target/release/flow-run
```

## 命令概览

```
flow-run [OPTIONS] <WORKFLOW_FILE> <COMMAND>

Commands:
  run         执行工作流
  resume      从检查点恢复工作流执行
  validate    验证工作流定义
  dry-run     模拟执行工作流（显示执行计划，不实际运行）
  checkpoint  检查点管理
  history     查看执行历史
  schema      输出工作流定义的 JSON Schema

Options:
  -v, --verbose          启用详细日志输出
  -C, --config <CONFIG>  指定配置文件
```

## 子命令详解

### run — 执行工作流

解析 YAML 工作流文件，构建 DAG 调度图，按依赖顺序执行所有步骤。

```bash
flow-run <workflow.yaml> run [OPTIONS]
```

**参数：**

| 参数 | 简写 | 说明 |
|:---|:---|:---|
| `--input <key=value>` | `-i` | 传入工作流输入参数，可多次使用 |
| `--json` | | 以 JSON 格式输出完整执行结果 |
| `--dry-run` | | 模拟执行，仅解析和展示执行计划 |
| `--normal` | | 普通执行模式（默认） |
| `--async-mode` | | 异步执行模式 |
| `--daemon` | | 守护进程模式 |

**示例：**

```bash
# 执行 HTTP 工作流
flow-run examples/01_basic_http.yaml run \
  --input api_url=https://jsonplaceholder.typicode.com

# 执行 Shell 工作流，传入多个参数
flow-run examples/02_basic_shell.yaml run \
  --input project_name=myapp \
  --input environment=production

# JSON 格式输出（适合程序解析）
flow-run examples/01_basic_http.yaml run \
  --input api_url=https://jsonplaceholder.typicode.com \
  --json

# 模拟执行（不实际运行步骤）
flow-run examples/02_basic_shell.yaml run --dry-run \
  --input project_name=myapp

# 启用详细日志
flow-run -v examples/01_basic_http.yaml run \
  --input api_url=https://jsonplaceholder.typicode.com
```

**输出示例（人可读）：**

```
执行结果: Success
步骤结果:
  [OK] fetch_user
  [OK] display_user  用户名: Bret, 邮箱: Sincere@april.biz

指标:
  总步骤: 2 | 成功: 2 | 失败: 0 | 跳过: 0
  耗时: 1835ms

工作流输出:
  user_name: "Bret"
  user_email: "Sincere@april.biz"
```

**退出码：**
- `0` — 工作流执行成功
- `1` — 工作流执行失败或 YAML 解析错误

---

### dry-run — 模拟执行

解析工作流文件，计算 DAG 拓扑排序，展示完整的执行分析报告，但不实际执行任何步骤。

```bash
flow-run <workflow.yaml> dry-run [OPTIONS]
```

**参数：**

| 参数 | 简写 | 说明 |
|:---|:---|:---|
| `--input <key=value>` | `-i` | 传入输入参数（仅用于展示） |
| `--json` | | 以 JSON 格式输出执行计划 |

**示例：**

```bash
flow-run examples/11_comprehensive_cicd.yaml dry-run
```

**输出内容：**

- 工作流基本信息（名称、版本、描述、步骤数、循环依赖检查）
- 全局配置（超时、失败策略、检查点、最大并发、重试策略）
- 输入参数定义（名称、类型、是否必填）和实际传入值
- 工作流输出定义和模板表达式
- 步骤列表（类型、依赖、超时、重试配置）
  - HTTP 步骤：显示 API 地址和方法
  - Shell 步骤：显示命令预览
  - Parallel 步骤：显示子步骤和最大并发数
  - Loop 步骤：显示循环配置
  - Condition 步骤：显示条件表达式和分支数
  - Workflow 步骤：显示子工作流路径
  - Approve 步骤：显示审批人列表
- DAG 结构（节点数、边数、所有依赖关系 `A ──→ B`）
- 拓扑排序执行计划（批次列表、并行标记、步骤出边）

**输出示例（CI/CD 工作流，关键部分）：**

```
══════════════════════════════════════════════
  Dry Run: CI/CD 完整流水线
══════════════════════════════════════════════
  描述: 一个完整的持续集成/持续部署工作流
  版本: 1.0.0
  步骤: 10 个
  DAG 检查: 无循环依赖

── 全局配置 ──
  超时: 30m
  失败策略: Pause
  最大并发: 4

── 拓扑排序（执行计划）──
  共 8 个批次
  批次 1: 1 个步骤
    ├─ checkoutShell - 检出代码 [out→ detect_changes]
  批次 2: 1 个步骤
    ├─ detect_changesShell - 检测变更 [out→ build_frontend, build_backend]
  批次 3: (并行) 2 个步骤
    ├─ build_frontendShell - 构建前端 [out→ test_parallel, security_scan]
    ├─ build_backendShell - 构建后端 [out→ test_parallel, security_scan]
  ...

── DAG 结构 ──
  节点: 10 | 边: 12
  checkout ──→ detect_changes
  detect_changes ──→ build_frontend
  detect_changes ──→ build_backend
  ...
```

---

### resume — 从检查点恢复

加载指定检查点，恢复工作流执行。适用于工作流中途失败（`on_failure: pause`）后，修复问题后从失败点继续执行。

```bash
flow-run <workflow.yaml> resume --checkpoint-id <ID> [OPTIONS]
```

**参数：**

| 参数 | 简写 | 说明 |
|:---|:---|:---|
| `--checkpoint-id <ID>` | | 要恢复的检查点 ID（必填） |
| `--input <key=value>` | `-i` | 覆盖输入参数 |
| `--json` | | 以 JSON 格式输出结果 |

**检查点目录：** 恢复操作在 `/tmp/flow-run-checkpoints` 下查找检查点文件。

**示例：**

```bash
# 从指定检查点恢复
flow-run examples/12_checkpoint_resume.yaml resume \
  --checkpoint-id cp_abc123
```

---

### validate — 验证工作流定义

检查 YAML 文件语法是否正确、DAG 是否存在循环依赖。

```bash
flow-run <workflow.yaml> validate [OPTIONS]
```

**参数：**

| 参数 | 说明 |
|:---|:---|
| `--show-dag` | 显示步骤列表和 DAG 结构 |
| `--json` | 以 JSON 格式输出工作流定义 |

**示例：**

```bash
# 验证工作流
flow-run examples/11_comprehensive_cicd.yaml validate

# 验证并显示 DAG 结构
flow-run examples/11_comprehensive_cicd.yaml validate --show-dag

# 输出完整 JSON 定义
flow-run examples/11_comprehensive_cicd.yaml validate --json
```

---

### checkpoint — 检查点管理

管理工作流执行过程中保存的检查点。

```bash
flow-run <workflow.yaml> checkpoint <ACTION>
```

**子命令：**

| 子命令 | 说明 |
|:---|:---|
| `list` | 列出所有检查点 |
| `show <ID>` | 显示检查点详情 |
| `clean` | 清理检查点 |

**list 参数：**

```bash
flow-run <workflow.yaml> checkpoint list [OPTIONS]
# --verbose, -v    显示详细信息
# --status <STATUS> 按状态过滤
# --json           JSON 格式输出
```

**show 参数：**

```bash
flow-run <workflow.yaml> checkpoint show <CHECKPOINT_ID> [OPTIONS]
# --steps, -s      显示步骤详情
# --json           JSON 格式输出
```

**clean 子命令：**

```bash
# 按 ID 清理
flow-run <workflow.yaml> checkpoint clean id <ID1> <ID2> ...

# 清理所有（需确认）
flow-run <workflow.yaml> checkpoint clean all --confirm

# 清理超过 N 天的
flow-run <workflow.yaml> checkpoint clean older-than --days 7

# 按状态清理
flow-run <workflow.yaml> checkpoint clean status <STATUS>

# 仅保留最近 N 个
flow-run <workflow.yaml> checkpoint clean keep --count 5
```

---

### history — 查看执行历史

```bash
flow-run <workflow.yaml> history [OPTIONS]
# --limit, -l <N>    最大显示条数（默认 20）
# --status <STATUS>  按状态过滤
# --failed           只显示失败的执行
# --json             JSON 格式输出
```

---

### schema — 输出 JSON Schema

输出工作流定义的 JSON Schema，用于编辑器自动补全和校验。

```bash
flow-run <workflow.yaml> schema [OPTIONS]
# --output, -o <PATH>  写入文件
# --pretty              美化输出
```

**示例：**

```bash
# 输出到终端
flow-run examples/01_basic_http.yaml schema --pretty

# 写入文件（供编辑器使用）
flow-run examples/01_basic_http.yaml schema -o workflow-schema.json
```

---

## 工作流 YAML 语法

### 基本结构

```yaml
name: "工作流名称"
description: "工作流描述"
version: "1.0.0"

inputs:
  - name: api_url
    type: string
    required: true

steps:
  - id: step_id
    name: "步骤名称"
    type: http          # http / shell / parallel / loop / condition / workflow / approve
    # ... 步骤配置

outputs:
  result_key: "${{ steps.step_id.output.path }}"
```

### 步骤类型

| 类型 | 说明 | 主要配置 |
|:---|:---|:---|
| `http` | HTTP 请求 | `api`, `method`, `headers`, `body` |
| `shell` | Shell 命令 | `run`, `env`, `safe_mode` |
| `parallel` | 并行执行 | `steps`, `max_concurrent` |
| `loop` | 循环执行 | `loop`, `do_steps` |
| `condition` | 条件分支 | `expression`, `then_steps`, `else_steps` |
| `workflow` | 子工作流 | `workflow`, `inputs`, `error_strategy` |
| `approve` | 人工审批 | `message`, `approvers`, `auto_approve_on` |

### 模板表达式

```yaml
# 变量引用
${{ inputs.variable_name }}
${{ steps.step_id.output_name }}
${{ variables.custom_var }}

# 路径访问
${{ steps.fetch.response.body.data }}
${{ steps.fetch.response.body.items[0].name }}

# 过滤器链
${{ steps.fetch.response.body.name | uppercase }}
${{ steps.fetch.response.body.name | truncate(10) }}
${{ variables.items | join(', ') }}

# 条件表达式
${{ inputs.env || 'development' }}
```

### 内置过滤器

| 过滤器 | 说明 | 示例 |
|:---|:---|:---|
| `uppercase` | 转大写 | `hello` → `HELLO` |
| `lowercase` | 转小写 | `HELLO` → `hello` |
| `trim` | 去空格 | ` hello ` → `hello` |
| `default(v)` | 默认值 | `null` → `v` |
| `length` | 长度 | `[1,2,3]` → `3` |
| `slice(s,e)` | 切片 | `[1,2,3] \| slice(0,2)` → `[1,2]` |
| `first` | 首元素 | `[1,2,3]` → `1` |
| `last` | 尾元素 | `[1,2,3]` → `3` |
| `join(sep)` | 拼接 | `[a,b] \| join('-')` → `a-b` |
| `split(sep)` | 分割 | `a-b \| split('-')` → `[a,b]` |
| `replace(o,n)` | 替换 | `hello \| replace(l,L)` → `heLLo` |
| `truncate(n)` | 截断 | `longtext \| truncate(5)` → `long...` |
| `to_json` | 转 JSON | `{a:1}` → `'{"a":1}'` |
| `from_json` | 解析 JSON | `'{"a":1}'` → `{a:1}` |

---

## 示例工作流

完整示例位于 `examples/` 目录：

```bash
# HTTP 请求
flow-run examples/01_basic_http.yaml run \
  --input api_url=https://jsonplaceholder.typicode.com

# Shell 命令
flow-run examples/02_basic_shell.yaml run \
  --input project_name=myapp --input environment=staging

# 步骤依赖
flow-run examples/03_basic_dependencies.yaml run

# 并行执行
flow-run examples/04_intermediate_parallel.yaml run

# 重试策略
flow-run examples/05_intermediate_retry.yaml run

# 模板表达式
flow-run examples/06_intermediate_templates.yaml run

# 循环执行
flow-run examples/07_advanced_loop.yaml run

# 条件分支
flow-run examples/08_advanced_condition.yaml run

# 子工作流
flow-run examples/09_advanced_subworkflow.yaml run

# 人工审批
flow-run examples/10_advanced_approval.yaml run

# CI/CD 流水线
flow-run examples/11_comprehensive_cicd.yaml run

# 检查点保存与恢复
flow-run examples/12_checkpoint_resume.yaml run
```

Rust 代码示例见 [`examples/README.md`](examples/README.md)。

## 环境变量

```bash
# 控制日志级别
RUST_LOG=debug flow-run examples/01_basic_http.yaml run
RUST_LOG=flow_run=trace flow-run examples/01_basic_http.yaml run
```
