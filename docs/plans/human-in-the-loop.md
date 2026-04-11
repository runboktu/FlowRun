# flow-run Human-in-the-Loop (HITL) 设计方案

> **版本**: v1.0 | **日期**: 2026-04-11 | **作者**: Sisyphus AI Architect
> **状态**: 设计阶段（待评审）

---

## 目录

1. [背景与目标](#1-背景与目标)
2. [现有基础](#2-现有基础)
3. [差距分析](#3-差距分析)
4. [设计方案](#4-设计方案)
5. [核心机制：Interrupt & Resume](#5-核心机制interrupt--resume)
6. [核心机制：Human Input 步骤类型](#6-核心机制human-input-步骤类型)
7. [核心机制：Tool Call 审批](#7-核心机制tool-call-审批)
8. [API 设计](#8-api-设计)
9. [YAML 语法设计](#9-yaml-语法设计)
10. [数据流与状态转换](#10-数据流与状态转换)
11. [实现计划](#11-实现计划)
12. [与 LangGraph HITL 的对比](#12-与-langgraph-hitl-的对比)

---

## 1. 背景与目标

### 1.1 什么是 Human-in-the-Loop？

Human-in-the-Loop (HITL) 是工作流在**关键节点暂停执行**，等待**人工干预**（审批、输入数据、修改结果）后**恢复执行**的机制。

### 1.2 典型场景

| 场景 | 描述 | 示例 |
|:---|:---|:---|
| **审批门控** | 关键步骤前暂停，等待人工批准 | 生产部署前需要运维经理审批 |
| **数据收集** | 暂停并询问人工补充信息 | Agent 遇到不确定信息，请求用户确认 |
| **结果审核** | 展示中间结果，人工可修改后继续 | LLM 生成文案，人工审核修改后发布 |
| **工具调用审批** | Agent 调用敏感工具前请求人工确认 | Agent 想执行 `rm -rf` 命令，需要人工批准 |

### 1.3 设计目标

1. **最小侵入**：基于现有的 checkpoint + approve 机制扩展，不重写核心调度
2. **YAML 声明式**：在 YAML 中声明 HITL 需求，不需要写代码
3. **多后端支持**：支持内存（测试）、文件系统、Redis、Webhook 等多种存储/通知后端
4. **类型安全**：所有 HITL 数据通过 serde 序列化，Rust 类型系统保障

---

## 2. 现有基础

flow-run 已经具备了实现 HITL 的**大部分基础设施**：

### 2.1 已有的 Approve 步骤 (`src/executors/approve.rs`)

```yaml
- id: deploy_approval
  type: approve
  message: "确认部署到生产环境？"
  approvers: ["ops_manager"]
  timeout: "30m"
  on_timeout: abort
  auto_approve_on:
    - condition: "${{ inputs.environment == 'staging' }}"
      reason: "测试环境自动批准"
```

**能力**：
- `ApprovalStore` trait — 可插拔的审批状态存储
- `ApprovalNotifier` trait — 可插拔的通知机制
- 轮询等待 (`wait_for_approval`，5 秒间隔)
- 超时策略 (`Abort` / `Pause` / `Continue`)
- 自动审批规则 (`auto_approve_on`)

**局限**：
- 只支持 approve/reject 二元决策
- 等待机制是**阻塞轮询**（线程空转）
- 人工无法提供结构化数据（只能 approve + comment）
- 没有 `WorkflowStatus::Paused` 的完整生命周期

### 2.2 已有的 Checkpoint 系统 (`src/utils/checkpoint.rs`)

```
Checkpoint {
    id, workflow_id, workflow_name,
    status: CheckpointStatus { Running, Paused, Completed, Failed },
    completed_steps: HashSet<StepId>,
    failed_steps: HashSet<StepId>,
    current_batch: usize,
    step_outputs: HashMap<StepId, StepResult>,
    variables: HashMap<String, Value>,
}
```

**能力**：
- JSON 文件持久化
- 保存完整执行状态（步骤输出 + 变量 + 批次进度）
- `CheckpointManager::save / load / list / delete`
- `Scheduler::resume(checkpoint_id)` 从 `current_batch + 1` 恢复

**局限**：
- `resume` 是全量恢复（重新执行后续所有批次），不支持恢复到某个特定步骤
- 没有 "pause reason" 概念 — 不知道为什么暂停了
- 没有 "resume payload" — 恢复时无法携带人工数据

### 2.3 已有的 Hooks (`HooksConfig`)

```yaml
on:
  workflow_pause:
    - http: { api: "https://notify.example.com/pause" }
  workflow_resume:
    - http: { api: "https://notify.example.com/resume" }
```

**已有暂停/恢复钩子** — 通知基础设施已就绪。

### 2.4 已有的 Status 枚举

```rust
WorkflowStatus { Success, Paused, Failed, DryRun }
CheckpointStatus { Running, Paused, Completed, Failed }
StepStatus { Success, Failed, Skipped, Running, Pending }
```

`WorkflowStatus::Paused` 和 `CheckpointStatus::Paused` **已经定义**，只是没有完整使用。

---

## 3. 差距分析

| 能力 | 现有状态 | 需要做的 |
|:---|:---|:---|
| 审批门控 | ✅ ApproveExecutor 已实现 | 增强：支持结构化响应 |
| 状态持久化 | ✅ Checkpoint 已实现 | 增强：添加 pause_reason, resume_payload |
| 暂停执行 | ⚠️ WorkflowStatus::Paused 已定义但未使用 | 实现：Scheduler 检测暂停并提前返回 |
| 恢复执行 | ✅ Scheduler::resume 已实现 | 增强：支持携带 resume payload |
| 人工输入 | ❌ 不存在 | 新增：`human_input` 步骤类型 |
| 工具审批 | ❌ 不存在 | 新增：Agent tool_call 审批拦截 |
| 事件通知 | ✅ Hooks 已支持 workflow_pause/resume | 无需修改 |

---

## 4. 设计方案

### 4.1 架构总览

```
                          ┌─────────────────────────┐
                          │       FlowRunner         │
                          │                          │
                          │  run() → 正常执行        │
                          │  run() → 命中 HITL →    │
                          │    save checkpoint       │
                          │    return Paused         │
                          │                          │
                          │  resume(id, payload) →   │
                          │    load checkpoint       │
                          │    inject payload        │
                          │    continue execution    │
                          └─────────────────────────┘
                                     │
                    ┌────────────────┼────────────────┐
                    ▼                ▼                ▼
             ┌──────────┐   ┌──────────┐   ┌──────────────┐
             │ Approve  │   │ Human    │   │ Tool Call    │
             │ 步骤     │   │ Input    │   │ 审批         │
             │ (已有)   │   │ 步骤     │   │ (Agent 层)   │
             │ 增强     │   │ (新增)   │   │ (新增)       │
             └──────────┘   └──────────┘   └──────────────┘
                    │                │                │
                    └────────────────┼────────────────┘
                                     ▼
                          ┌─────────────────────────┐
                          │    InterruptManager      │
                          │                          │
                          │  pause() → 保存状态      │
                          │  resume() → 注入数据     │
                          │  wait() → 事件驱动等待   │
                          └─────────────────────────┘
                                     │
                                     ▼
                          ┌─────────────────────────┐
                          │    InterruptStore        │
                          │    (trait, 可插拔)       │
                          │                          │
                          │  InMemory (测试)         │
                          │  FileStore (单机)        │
                          │  RedisStore (生产)       │
                          └─────────────────────────┘
```

### 4.2 三种 HITL 模式

#### 模式 A：Approve 门控（已有，增强）

工作流在某个步骤**之前或之后**暂停，等待人工批准/拒绝。

```
step_1 (完成) → [approve_step: 暂停等待] → step_2 (继续或中止)
```

**增强点**：
- 审批人可以返回**结构化数据**（不仅是 approve/reject）
- 支持 `on_reject` 分支（拒绝后执行备选步骤）
- 非阻塞等待（从轮询改为 tokio::Notify 事件通知）

#### 模式 B：Human Input 数据收集（新增）

工作流暂停，等待人工提供数据（文本、选择、表单），数据注入后继续执行。

```
step_1 (完成) → [human_input: 请求人工输入] → step_2 (使用人工输入)
```

**典型场景**：
- Agent 分析后需要用户确认："发现 3 种方案，请选择"
- 缺少必要信息时请求用户补充
- A/B 测试需要人工选择方向

#### 模式 C：Agent Tool Call 审批（新增）

Agent 执行过程中，对敏感工具调用进行拦截，等待人工批准后执行。

```
Agent 推理 → [想调用 shell_exec("rm -rf /")] → 暂停 → 人工审批 → 执行或拒绝
```

**典型场景**：
- Agent 想执行破坏性 shell 命令
- Agent 想发送邮件/消息
- Agent 想调用付费 API

---

## 5. 核心机制：Interrupt & Resume

### 5.1 InterruptManager（新增）

```rust
/// 中断管理器 — HITL 的核心调度组件
pub struct InterruptManager {
    store: Arc<dyn InterruptStore>,
}

/// 中断记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterruptRecord {
    /// 唯一 ID
    pub id: String,
    /// 工作流执行 ID
    pub execution_id: String,
    /// 触发中断的步骤 ID
    pub step_id: String,
    /// 中断类型
    pub interrupt_type: InterruptType,
    /// 中断原因（展示给人工的描述）
    pub reason: String,
    /// 展示给人工的上下文数据
    pub context: serde_json::Value,
    /// 期望人工返回的数据 schema
    pub expected_response: Option<JsonSchema>,
    /// 创建时间
    pub created_at: DateTime<Utc>,
    /// 解决时间
    pub resolved_at: Option<DateTime<Utc>>,
    /// 状态
    pub status: InterruptStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InterruptType {
    /// 审批门控
    Approval,
    /// 数据收集
    HumanInput,
    /// 工具调用审批
    ToolApproval,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InterruptStatus {
    /// 等待人工响应
    Pending,
    /// 人工已批准/提交
    Resolved,
    /// 人工已拒绝
    Rejected,
    /// 超时
    Expired,
}

/// 中断存储 trait
#[async_trait]
pub trait InterruptStore: Send + Sync {
    /// 创建中断记录
    async fn create(&self, record: InterruptRecord) -> Result<(), WorkflowError>;
    /// 获取中断记录
    async fn get(&self, id: &str) -> Result<Option<InterruptRecord>, WorkflowError>;
    /// 解决中断（人工提交响应）
    async fn resolve(&self, id: &str, response: HumanResponse) -> Result<(), WorkflowError>;
    /// 按执行 ID 查询所有待处理中断
    async fn list_pending(&self, execution_id: &str) -> Result<Vec<InterruptRecord>, WorkflowError>;
}

/// 人工响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanResponse {
    /// 响应动作
    pub action: ResponseAction,
    /// 响应数据（人工填写的表单、选择等）
    pub data: serde_json::Value,
    /// 审批人
    pub responder: Option<String>,
    /// 备注
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponseAction {
    /// 批准 / 提交
    Approve,
    /// 拒绝
    Reject,
    /// 修改后提交
    Edit,
}
```

### 5.2 暂停流程

```
1. Scheduler 执行步骤 → 检测到 HITL 步骤
2. 创建 InterruptRecord → 存入 InterruptStore
3. 保存 Checkpoint（status = Paused, pause_reason = "hitl:step_id"）
4. 触发 workflow_pause hook
5. 返回 WorkflowResult { status: Paused, ... }
   ↓
   工作流进程可以安全退出（状态已持久化）
```

### 5.3 恢复流程

```
1. 人工通过 API/CLI 提交响应 → InterruptStore.resolve()
2. FlowRunner.resume(checkpoint_id, payload) 被调用
3. 加载 Checkpoint + InterruptRecord
4. 将 HumanResponse.data 注入 ExecutionContext
5. 触发 workflow_resume hook
6. 从暂停点继续执行后续步骤
```

### 5.4 非阻塞等待（替代轮询）

现有 `ApproveExecutor::wait_for_approval` 使用 5 秒轮询。改为事件驱动：

```rust
/// 事件驱动的中断等待
pub struct InterruptWaiter {
    notify: Arc<tokio::sync::Notify>,
    store: Arc<dyn InterruptStore>,
}

impl InterruptWaiter {
    /// 等待中断解决（非阻塞，可被 timeout 取消）
    pub async fn wait(&self, interrupt_id: &str, timeout: Option<Duration>) -> Result<HumanResponse, WorkflowError> {
        match timeout {
            Some(dur) => {
                tokio::time::timeout(dur, self.wait_inner(interrupt_id))
                    .await
                    .map_err(|_| WorkflowError::ApprovalTimeout { step_id: interrupt_id.to_string() })?
            }
            None => self.wait_inner(interrupt_id).await,
        }
    }

    async fn wait_inner(&self, interrupt_id: &str) -> Result<HumanResponse, WorkflowError> {
        loop {
            if let Some(record) = self.store.get(interrupt_id).await? {
                if record.status != InterruptStatus::Pending {
                    return record.human_response.unwrap();
                }
            }
            self.notify.notified().await;
        }
    }
}
```

**但更推荐的方案是直接返回 Paused**：

```rust
// 不等待，直接暂停工作流
// 让调用者决定何时恢复
// 这是 LangGraph 的做法 — interrupt() 不阻塞，直接返回
```

---

## 6. 核心机制：Human Input 步骤类型

### 6.1 新增 StepType::HumanInput

```rust
// types.rs — 新增
pub enum StepType {
    Http,
    Shell,
    Parallel,
    Loop,
    Condition,
    Workflow,
    Approve,
    Agent,
    Tool,
    HumanInput,  // ← 新增
}
```

### 6.2 StepDefinition 新增字段

```rust
pub struct StepDefinition {
    // ... 现有字段 ...

    // Human Input 步骤特有字段
    pub prompt: Option<String>,                    // 展示给人工的问题/提示
    pub input_type: Option<HumanInputType>,        // 期望的输入类型
    pub choices: Option<Vec<Choice>>,              // 选择题选项
    pub default_value: Option<serde_json::Value>,  // 默认值
    pub validation: Option<ValidationRule>,        // 输入验证规则
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HumanInputType {
    /// 自由文本
    Text,
    /// 单选
    SingleChoice,
    /// 多选
    MultiChoice,
    /// 数值
    Number,
    /// 确认（是/否）
    Confirm,
    /// 表单（结构化数据）
    Form {
        fields: Vec<FormFields>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    pub label: String,
    pub value: serde_json::Value,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationRule {
    pub required: Option<bool>,
    pub min_length: Option<usize>,
    pub max_length: Option<usize>,
    pub pattern: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormField {
    pub name: String,
    pub label: String,
    pub field_type: String,
    pub required: Option<bool>,
    pub default: Option<serde_json::Value>,
}
```

### 6.3 HumanInputExecutor

```rust
pub struct HumanInputExecutor {
    interrupt_manager: Arc<InterruptManager>,
}

impl HumanInputExecutor {
    pub async fn execute(
        &self,
        step: &StepDefinition,
        context: &ExecutionContext,
    ) -> Result<StepResult, WorkflowError> {
        let step_id = &step.id;

        // 1. 构建 prompt（模板解析）
        let prompt = self.resolve_prompt(step, context)?;

        // 2. 创建中断记录
        let record = InterruptRecord {
            id: uuid::Uuid::new_v4().to_string(),
            execution_id: context.execution_id.clone(),
            step_id: step_id.clone(),
            interrupt_type: InterruptType::HumanInput,
            reason: prompt,
            context: self.build_context_data(step, context),
            expected_response: step.input_type.as_ref().map(|t| t.to_json_schema()),
            created_at: Utc::now(),
            resolved_at: None,
            status: InterruptStatus::Pending,
        };

        self.interrupt_manager.store.create(record).await?;

        // 3. 返回 Paused 状态 — 工作流将暂停
        Ok(StepResult {
            step_id: step_id.clone(),
            status: StepStatus::Pending,  // Pending = 等待人工
            started_at: Utc::now(),
            completed_at: None,
            duration_ms: None,
            output: Some(serde_json::json!({
                "interrupt_id": record.id,
                "status": "waiting_for_human",
                "prompt": prompt,
            })),
            error: None,
        })
    }
}
```

---

## 7. 核心机制：Tool Call 审批

### 7.1 设计思路

Agent 执行过程中，某些工具调用需要人工确认。这是在 Agent 层面的拦截：

```
Agent LLM → 产出 tool_call → [拦截检查] → 需要审批？
                                        ├── 否 → 正常执行工具
                                        └── 是 → 暂停工作流 → 人工审批 → 继续或拒绝
```

### 7.2 YAML 声明

```yaml
- id: research_agent
  type: agent
  agent_system_prompt: "你是研究员"
  agent_tools:
    - name: web_search
      source: builtin
    - name: shell_exec
      source: builtin
      require_approval: true                    # ← 新增：该工具需要人工审批
      approval_message: "Agent 请求执行命令: ${{ tool_args.command }}"
  agent_input: "${{ inputs.question }}"
  agent_stream: true
  agent_require_tool_approval: true             # ← 全局开关：所有工具都需要审批
```

### 7.3 实现：在 AgentExecutor 中拦截

```rust
// agent_executor.rs — 修改 execute_agent_tool_call

async fn execute_tool_call(&self, step: &StepDefinition, tool_call: &ToolCall) -> Result<ToolResult, WorkflowError> {
    // 检查是否需要人工审批
    if self.needs_approval(step, &tool_call.name).await? {
        let record = self.create_tool_approval_interrupt(step, tool_call).await?;

        // 返回暂停状态，工作流将在 checkpoint 中保存
        return Err(WorkflowError::InterruptRequired {
            step_id: step.id.clone(),
            interrupt_id: record.id,
            tool_name: tool_call.name.clone(),
            tool_args: tool_call.arguments.clone(),
        });
    }

    // 正常执行工具
    self.tool_registry.execute(&tool_call.name, &tool_call.arguments).await
}
```

---

## 8. API 设计

### 8.1 FlowRunner API（增强）

```rust
impl FlowRunner {
    /// 执行工作流 — 可能返回 Paused
    pub async fn run(&self, inputs: HashMap<String, Value>) -> Result<WorkflowResult, WorkflowError>;

    /// 恢复执行 — 携带人工响应
    pub async fn resume(
        &self,
        checkpoint_id: &str,
        payload: ResumePayload,
    ) -> Result<WorkflowResult, WorkflowError>;

    /// 查询待处理的中断
    pub async fn pending_interrupts(&self, execution_id: &str) -> Result<Vec<InterruptRecord>, WorkflowError>;

    /// 提交人工响应（不恢复执行，仅记录响应）
    pub async fn submit_response(
        &self,
        interrupt_id: &str,
        response: HumanResponse,
    ) -> Result<(), WorkflowError>;

    /// 恢复执行（使用已提交的响应）
    pub async fn resume_with_response(
        &self,
        checkpoint_id: &str,
    ) -> Result<WorkflowResult, WorkflowError>;
}

/// 恢复载荷
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumePayload {
    /// 中断 ID
    pub interrupt_id: String,
    /// 人工响应
    pub response: HumanResponse,
}
```

### 8.2 调用示例

```rust
// 1. 正常执行 — 遇到 human_input 步骤时暂停
let result = runner.run(inputs).await?;
// result.status == WorkflowStatus::Paused
// result.execution.checkpoint == Some("abc-123")
let checkpoint_id = result.execution.checkpoint.unwrap();
let interrupt_id = result.steps.last().unwrap().output.as_ref()
    .unwrap()["interrupt_id"].as_str().unwrap();

// 2a. 方案 A：直接 resume（一步完成提交 + 恢复）
let payload = ResumePayload {
    interrupt_id: interrupt_id.to_string(),
    response: HumanResponse {
        action: ResponseAction::Approve,
        data: serde_json::json!("选择方案 B"),
        responder: Some("user_001".to_string()),
        comment: None,
    },
};
let result = runner.resume(checkpoint_id, payload).await?;

// 2b. 方案 B：先提交，后恢复（适合异步场景）
runner.submit_response(interrupt_id, response).await?;
// ... 人工可以稍后再恢复
let result = runner.resume_with_response(checkpoint_id).await?;
```

---

## 9. YAML 语法设计

### 9.1 审批门控（已有，增强）

```yaml
steps:
  - id: deploy
    type: shell
    run: "kubectl apply -f production.yaml"

  - id: deploy_approval
    type: approve
    depends_on: [deploy]
    message: "部署已完成，确认无异常？"
    approvers: ["ops_manager", "tech_lead"]
    require_comment: true
    timeout: "30m"
    on_timeout: abort
    on_reject:                  # ← 新增：拒绝后的处理
      steps:
        - id: rollback
          type: shell
          run: "kubectl rollout undo deployment/app"
```

### 9.2 Human Input（新增）

```yaml
steps:
  - id: agent_analysis
    type: agent
    agent_system_prompt: "分析用户需求，给出至少 2 种方案"
    agent_input: "${{ inputs.question }}"
    agent_stream: true

  - id: user_choice
    type: human_input
    depends_on: [agent_analysis]
    prompt: |
      Agent 分析完成。以下是方案概要：
      ${{ steps.agent_analysis.answer }}

      请选择你要执行的方案：
    input_type: single_choice
    choices:
      - label: "方案 A（快速迭代）"
        value: "plan_a"
        description: "先发布 MVP，后续迭代"
      - label: "方案 B（完整交付）"
        value: "plan_b"
        description: "完整实现所有功能后发布"
      - label: "方案 C（放弃）"
        value: "cancel"
    timeout: "24h"
    on_timeout: abort

  - id: execute_plan
    type: agent
    depends_on: [user_choice]
    agent_system_prompt: "根据用户选择的方案执行实现"
    agent_input: |
      用户选择了：${{ steps.user_choice.response.data }}
      原始分析：${{ steps.agent_analysis.answer }}
```

### 9.3 简化的确认（快捷语法）

```yaml
steps:
  - id: confirm_proceed
    type: human_input
    prompt: "确认继续执行？"
    input_type: confirm
    timeout: "5m"
```

### 9.4 表单收集

```yaml
steps:
  - id: collect_config
    type: human_input
    prompt: "请提供部署配置"
    input_type: form
    fields:
      - name: environment
        label: "目标环境"
        type: single_choice
        choices: ["staging", "production"]
        required: true
      - name: replicas
        label: "副本数"
        type: number
        default: 3
      - name: notify_email
        label: "通知邮箱"
        type: text
```

### 9.5 Agent 工具审批

```yaml
steps:
  - id: coding_agent
    type: agent
    agent_system_prompt: "你是全栈工程师"
    agent_tools:
      - name: read_file
        source: builtin
      - name: write_file
        source: builtin
        require_approval: true
        approval_prompt: "Agent 请求修改文件: ${{ tool_args.path }}"
      - name: shell_exec
        source: builtin
        require_approval: true
        approval_prompt: "Agent 请求执行: ${{ tool_args.command }}"
    agent_input: "${{ inputs.task }}"
    agent_stream: true
```

---

## 10. 数据流与状态转换

### 10.1 完整的 HITL 执行流程

```
┌─────────────────────────────────────────────────────────────────┐
│                        FlowRunner.run()                         │
│                                                                  │
│  Batch 1: [step_1] ──→ Success                                  │
│  Batch 2: [human_input_step] ──→ 检测到 HITL                    │
│       │                                                          │
│       ├── 创建 InterruptRecord                                   │
│       ├── 创建 Checkpoint (status=Paused)                        │
│       ├── 触发 workflow_pause hook                               │
│       └── 返回 WorkflowResult { status: Paused, checkpoint }     │
│                                                                  │
│  ═══════════ 工作流进程退出 ═══════════                          │
│  ═══════════ 人工通过 API/CLI 提交响应 ═══════════               │
│  ═══════════ FlowRunner.resume() 被调用 ═══════════              │
│                                                                  │
│  Batch 2 (继续): [human_input_step]                              │
│       ├── 从 InterruptStore 读取 HumanResponse                   │
│       ├── 构造 StepResult { output: human_data }                 │
│       ├── 写入 step_outputs["human_input_step"]                  │
│       └── 继续后续 batch                                         │
│                                                                  │
│  Batch 3: [step_3] ──→ 模板引用 ${{ steps.human_input_step.response.data }} │
│  Batch 4: ...                                                    │
│                                                                  │
│  返回 WorkflowResult { status: Success }                         │
└─────────────────────────────────────────────────────────────────┘
```

### 10.2 状态转换图

```
WorkflowStatus:
  [Started] → Running → Success
                    ↓
                  Paused ←──── resume() ────→ Running → Success
                    ↓                              ↓
                  Failed                        Failed

StepStatus (HITL 步骤):
  [Pending] → Running → Waiting (等待人工)
                         ↓
                       Resolved → Success (人工批准)
                                → Failed (人工拒绝)
                         ↓
                       Expired → Failed (超时)

InterruptStatus:
  [Pending] → Resolved (人工批准)
            → Rejected (人工拒绝)
            → Expired (超时)
```

### 10.3 Checkpoint 增强

```rust
/// 增强后的 Checkpoint
pub struct Checkpoint {
    // ... 现有字段 ...

    /// 暂停原因
    pub pause_reason: Option<PauseReason>,
    /// 暂停时等待的中断 ID
    pub pending_interrupt_id: Option<String>,
    /// 暂停时等待的步骤 ID
    pub pending_step_id: Option<String>,
    /// 人工响应数据（恢复时注入）
    pub human_response: Option<HumanResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PauseReason {
    /// 等待人工审批
    Approval { step_id: String },
    /// 等待人工输入
    HumanInput { step_id: String },
    /// 等待工具审批
    ToolApproval { step_id: String, tool_name: String },
    /// 手动暂停
    Manual,
    /// 失败暂停
    OnFailure,
}
```

---

## 11. 实现计划

### Phase 1：核心框架（基础设施）

**修改文件**：
- `src/core/types.rs` — 新增 `StepType::HumanInput`、`HumanInputType`、`PauseReason`、`HumanResponse` 等类型
- `src/core/types.rs` — `StepDefinition` 新增 `prompt`、`input_type`、`choices`、`fields` 字段
- `src/utils/checkpoint.rs` — `Checkpoint` 新增 `pause_reason`、`pending_interrupt_id`、`human_response` 字段
- `src/executors/mod.rs` — 注册 `HumanInputExecutor`

**新增文件**：
- `src/executors/interrupt.rs` — `InterruptManager`、`InterruptRecord`、`InterruptStore` trait、`InMemoryInterruptStore`
- `src/executors/human_input.rs` — `HumanInputExecutor`

**工作量**：约 3-4 小时

### Phase 2：Scheduler 集成（暂停/恢复逻辑）

**修改文件**：
- `src/core/dag.rs` — `Scheduler::execute_step` 检测 HITL 步骤，返回 Paused
- `src/core/dag.rs` — `Scheduler::resume` 增强：支持注入 `HumanResponse`
- `src/core/dag.rs` — 暂停时保存增强版 Checkpoint（含 pause_reason）
- `src/core/runner.rs` — `FlowRunner::resume` 增加 `ResumePayload` 参数
- `src/core/runner.rs` — 新增 `FlowRunner::submit_response`、`pending_interrupts`

**工作量**：约 4-5 小时

### Phase 3：YAML 解析 & 验证

**修改文件**：
- `src/core/parser.rs` — 解析 `human_input` 步骤的新字段
- `src/core/parser.rs` — 验证 `human_input` 步骤的 schema
- `src/core/parser.rs` — 解析 `agent_tools[].require_approval`

**工作量**：约 2-3 小时

### Phase 4：Agent 工具审批

**修改文件**：
- `src/executors/agent_executor.rs` — 工具调用前检查 `require_approval`
- `src/agent/react_agent.rs` — 工具调用拦截点

**工作量**：约 3-4 小时

### Phase 5：示例 & 文档

- `examples/22_human_input.yaml` — Human Input 示例
- `examples/22_tool_approval.yaml` — Agent 工具审批示例
- `examples/code/22_human_input.rs` — Rust example
- 更新 `docs/` 文档

**工作量**：约 2 小时

---

## 12. 与 LangGraph HITL 的对比

| 维度 | LangGraph | flow-run (设计后) |
|:---|:---|:---|
| **暂停机制** | `interrupt()` 函数 + Checkpoint | `human_input` 步骤类型 + Checkpoint |
| **恢复机制** | `Command(resume=value)` | `FlowRunner.resume(id, payload)` |
| **审批** | 无内置，需手动在节点中实现 | 内置 `approve` 步骤类型 |
| **数据收集** | `interrupt()` + 自由格式 | 内置 `human_input`，支持多种输入类型 |
| **工具审批** | 无内置，需包装工具 | `require_approval: true` 声明式 |
| **状态持久化** | Checkpointer (Memory/SQLite/Postgres) | CheckpointManager (File) + 可扩展 |
| **等待方式** | 不等待，直接返回 | 不等待，直接返回 `Paused` |
| **声明式** | 否（Python 代码控制） | 是（YAML 声明） |

### 设计哲学差异

**LangGraph**：代码优先。`interrupt()` 是一个 Python 函数调用，灵活但需要写代码。

**flow-run**：声明优先。在 YAML 中声明 HITL 需求，引擎自动处理暂停/恢复。适合 DevOps/CI/CD 场景，人工通过 API 交互。

---

*文档版本: v1.0*
*最后更新: 2026-04-11*
