# flow-run 示例目录

本目录包含 flow-run 工作流引擎的使用示例，从基础到高级，逐步展示各项功能。

## 目录结构

```
examples/
├── README.md                          # 本文件
├── 01_basic_http.yaml                 # 基础 - HTTP 请求
├── 02_basic_shell.yaml                # 基础 - Shell 命令
├── 03_basic_dependencies.yaml         # 基础 - 步骤依赖
├── 04_intermediate_parallel.yaml      # 中级 - 并行执行
├── 05_intermediate_retry.yaml         # 中级 - 错误处理与重试
├── 06_intermediate_templates.yaml     # 中级 - 模板表达式
├── 07_advanced_loop.yaml              # 高级 - 循环执行
├── 08_advanced_condition.yaml         # 高级 - 条件分支
├── 09_advanced_subworkflow.yaml       # 高级 - 子工作流
├── 10_advanced_approval.yaml          # 高级 - 人工审批
├── 11_comprehensive_cicd.yaml         # 综合 - CI/CD 流水线
└── 12_checkpoint_resume.yaml          # 综合 - 检查点保存与恢复
```

## 使用方法

```bash
# 运行基础示例
flow-run examples/01_basic_http.yaml run --input api_url=https://jsonplaceholder.typicode.com

# 验证工作流
flow-run examples/01_basic_http.yaml validate

# 试运行（不实际执行）
flow-run examples/01_basic_http.yaml dry-run

# 带参数运行
flow-run examples/02_basic_shell.yaml run \
  --input project_name=myapp \
  --input environment=production

# JSON 输出
flow-run examples/03_basic_dependencies.yaml run \
  --input source_url=https://api.example.com/data \
  --input target_path=/tmp/output.json \
  --json
```

## 示例说明

### 基础示例

| 示例 | 说明 | 关键功能 |
|:---|:---|:---|
| `01_basic_http.yaml` | 简单 HTTP GET 请求 | HTTP 步骤、模板表达式、步骤输出引用 |
| `02_basic_shell.yaml` | Shell 命令执行 | Shell 步骤、环境变量、命令输出 |
| `03_basic_dependencies.yaml` | 步骤依赖和数据流 | depends_on、DAG 并行调度 |

### 中级示例

| 示例 | 说明 | 关键功能 |
|:---|:---|:---|
| `04_intermediate_parallel.yaml` | 并行执行多个任务 | parallel 类型、max_concurrent |
| `05_intermediate_retry.yaml` | 错误处理和重试 | retry 配置、退避策略、expect 验证 |
| `06_intermediate_templates.yaml` | 模板表达式和过滤器 | 过滤器链、条件表达式、数组操作 |

### 高级示例

| 示例 | 说明 | 关键功能 |
|:---|:---|:---|
| `07_advanced_loop.yaml` | 循环执行 | ForEach/While/Range 循环 |
| `08_advanced_condition.yaml` | 条件分支 | if/else 条件、动态路由 |
| `09_advanced_subworkflow.yaml` | 子工作流组合 | workflow 类型、模块化设计 |
| `10_advanced_approval.yaml` | 人工审批 | approve 类型、自动审批条件 |

### 综合示例

| 示例 | 说明 | 关键功能 |
|:---|:---|:---|
| `11_comprehensive_cicd.yaml` | 完整 CI/CD 流水线 | 综合使用所有功能 |

## 模板表达式语法

```yaml
# 基本变量引用
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
${{ inputs.skip_tests == false }}
```

## 内置过滤器

| 过滤器 | 说明 | 示例 |
|:---|:---|:---|
| `uppercase` | 转大写 | `hello` -> `HELLO` |
| `lowercase` | 转小写 | `HELLO` -> `hello` |
| `trim` | 去除空格 | ` hello ` -> `hello` |
| `default(v)` | 默认值 | `null` -> `v` |
| `length` | 长度 | `[1,2,3]` -> `3` |
| `slice(s,e)` | 切片 | `[1,2,3] \| slice(0,2)` -> `[1,2]` |
| `first` | 首元素 | `[1,2,3]` -> `1` |
| `last` | 尾元素 | `[1,2,3]` -> `3` |
| `join(sep)` | 拼接 | `[a,b] \| join('-')` -> `a-b` |
| `split(sep)` | 分割 | `a-b \| split('-')` -> `[a,b]` |
| `replace(o,n)` | 替换 | `hello \| replace(l,L)` -> `heLLo` |
| `truncate(n)` | 截断 | `longtext \| truncate(5)` -> `long...` |
| `to_json` | 转 JSON | `{a:1}` -> `'{"a":1}'` |
| `from_json` | 解析 JSON | `'{"a":1}'` -> `{a:1}` |

## 配置选项

```yaml
config:
  # 全局超时
  timeout: "30m"

  # 失败策略: abort/pause/continue
  on_failure: pause

  # 检查点文件
  checkpoint: "/tmp/checkpoint.json"

  # 最大并发数
  max_concurrent: 4

  # 超时策略: hard/soft
  timeout_strategy: hard
```

## 步骤类型

| 类型 | 说明 | 主要参数 |
|:---|:---|:---|
| `http` | HTTP 请求 | api, method, headers, body |
| `shell` | Shell 命令 | run, env, safe_mode |
| `parallel` | 并行执行 | steps, max_concurrent |
| `loop` | 循环 | loop, do_steps |
| `condition` | 条件分支 | expression, then_steps, else_steps |
| `workflow` | 子工作流 | workflow, inputs, error_strategy |
| `approve` | 人工审批 | message, approvers, auto_approve_on |

## Rust 代码示例

`code/` 目录包含两类 Rust 代码示例：

### API 演示示例

展示 flow-run 各个核心 API 的独立用法。

```bash
cargo run --example 01_load_workflow
cargo run --example 03_dag_scheduler
cargo run --example 04_template_engine
cargo run --example 05_retry_engine
cargo run --example 07_full_execution
```

| 示例 | 说明 | 关键 API |
|:---|:---|:---|
| `01_load_workflow` | 加载 YAML 工作流文件 | `WorkflowParser::from_file`, `from_str` |
| `03_dag_scheduler` | DAG 调度器使用 | `DagScheduler::new`, `topological_sort`, `has_cycle` |
| `04_template_engine` | 模板表达式引擎 | `TemplateEngine::new`, `evaluate`, `resolve_template` |
| `05_retry_engine` | 重试引擎使用 | `RetryEngine::new`, `execute`, `calculate_delay` |
| `07_full_execution` | 完整工作流执行 | 综合使用所有 API |

### YAML 工作流示例

加载对应的 YAML 工作流文件并完整执行，适合学习工作流引擎的端到端使用。

```bash
cargo run --example 02_basic_shell          # Shell 命令执行
cargo run --example 03_step_dependencies    # 步骤依赖和数据流
cargo run --example 04_parallel_execution   # 并行执行与限流
cargo run --example 05_retry_strategy       # 错误处理与重试
cargo run --example 06_template_filters     # 模板表达式与过滤器
cargo run --example 07_full_execution       # 循环执行
cargo run --example 08_condition_branch     # 条件分支
cargo run --example 09_subworkflow          # 子工作流
cargo run --example 10_advanced_approval    # 人工审批
cargo run --example 11_comprehensive_cicd   # 完整 CI/CD 流水线
cargo run --example 12_checkpoint_resume    # 检查点保存与恢复
```

| 示例 | 对应 YAML | 说明 |
|:---|:---|:---|
| `02_basic_shell` | `02_basic_shell.yaml` | Shell 命令执行、步骤依赖、命令输出 |
| `03_step_dependencies` | `03_basic_dependencies.yaml` | DAG 自动并行调度、步骤间数据传递 |
| `04_parallel_execution` | `04_intermediate_parallel.yaml` | parallel 类型、max_concurrent 限流 |
| `05_retry_strategy` | `05_intermediate_retry.yaml` | 重试策略、退避算法、expect 验证 |
| `06_template_filters` | `06_intermediate_templates.yaml` | 过滤器链、条件表达式、数组操作 |
| `07_full_execution` | `07_advanced_loop.yaml` | 循环执行 |
| `08_condition_branch` | `08_advanced_condition.yaml` | 条件分支 |
| `09_subworkflow` | `09_advanced_subworkflow.yaml` | 子工作流组合、模块化设计 |
| `10_advanced_approval` | `10_advanced_approval.yaml` | 人工审批、自动审批条件 |
| `11_comprehensive_cicd` | `11_comprehensive_cicd.yaml` | 完整 CI/CD 流水线 |
| `12_checkpoint_resume` | `12_checkpoint_resume.yaml` | 检查点保存、失败暂停、从检查点恢复 |
