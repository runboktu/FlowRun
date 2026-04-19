# 人工审阅循环 + Agent Session 复用方案设计

> 日期：2026-04-25
> 状态：草案
> 关联工作流：doc-to-video.yaml

---

## 目录

1. [背景与问题](#1-背景与问题)
2. [现有机制能力边界分析](#2-现有机制能力边界分析)
3. [方案设计：Review Loop Pattern](#3-方案设计review-loop-pattern)
4. [方案设计：Agent Session 复用](#4-方案设计agent-session-复用)
5. [改动清单](#5-改动清单)
6. [完整 YAML 示例](#6-完整-yaml-示例)
7. [风险与边界情况](#7-风险与边界情况)

---

## 1. 背景与问题

### 1.1 当前工作流

doc-to-video 工作流中，`generate_prompts` 步骤由 AI Agent 根据文章内容、SRT 字幕时间戳和视觉风格生成 `prompts.json`（分镜头脚本）。生成后直接写入文件，用户没有机会审阅和修改。

```
setup → clean_text → tts → get_duration + read_md + read_srt
                                    ↓
                           generate_prompts → write_prompts → gen_images → ...
```

### 1.2 需求

1. **展示** — 将 AI 生成的分镜头脚本展示给用户
2. **决策** — 用户满意 → 继续；不满意 → 提供修改意见
3. **循环** — 将用户反馈注入下一轮 `generate_prompts`，重新生成
4. **会话连续** — Agent 保留历史对话，LLM 能看到自己之前的输出 + 用户反馈，而非从零开始
5. **直到满意** — 循环终止，继续后续步骤

---

## 2. 现有机制能力边界分析

### 2.1 Agent 当前生命周期

```
AgentExecutor::execute()
  │
  ├─ 1. create_session_with_llm()  → 新建 session
  ├─ 2. register_tool()            → 注册工具
  ├─ 3. run_sync_stream()          → 执行推理
  │      └─ run_internal_stream()
  │           ├─ self.messages.clear()   ← 💀 每次调用先清空历史
  │           ├─ push system_prompt
  │           ├─ push user_input
  │           ├─ LLM call + tool loop
  │           └─ return final_answer
  │
  └─ 4. destroy_session()          ← 💀 立即销毁 session
```

**两个问题**：

| # | 位置 | 问题 |
|---|------|------|
| 1 | `agent_executor.rs:90` | `destroy_session(&session_id)` — session 用完即毁，历史全部丢失 |
| 2 | `react_agent.rs:108/155/258` | `self.messages.clear()` — 每个 `run_*` 方法开头都清空消息 |

**好消息**：底层设施已经支持会话保持，只是上层没用起来：

- `AgentManager.sessions` 是 `HashMap<String, ReActAgent>` — session 可以长期存活
- `ReActAgent.messages: Vec<Message>` — 消息会持续累积（如果没有被 clear）
- `AgentManager` 已有 `session_exists()`, `clear_history()` 等管理方法

### 2.2 Loop / Approve 现有能力

| 现有机制 | 能力 | 缺口 |
|---------|------|------|
| `approve` step | 阻塞等待人工决策 | 拒绝=错误（`WorkflowError::ApprovalRejected`），无法作为「继续循环」的信号；无结构化反馈收集 |
| `while` loop | 条件循环 | **前置检查**（pre-test），条件在循环体之前求值；循环上下文是克隆的，子步骤修改变量**不回传**到父上下文 |
| `loop` context | 每次迭代独立上下文 | 不支持从循环体内部修改变量来影响循环条件 |
| checkpoint/resume | 暂停恢复 | 多次手动 CLI 命令，UX 差 |

---

## 3. 方案设计：Review Loop Pattern

### 3.1 总体思路

使用 `while` 循环包裹 `generate_prompts` + `review_prompts` 两个步骤，通过 `loop.last` 上下文注入实现迭代间数据传递。

### 3.2 新增 Step Type: `review`

#### 3.2.1 定位

`review` 与 `approve` 的区别：

| | `approve` | `review` |
|---|---|---|
| 目的 | 门控审批（部署/删除等高风险操作） | 内容审阅（AI 生成物质量确认） |
| reject 语义 | 错误，终止流程 | 正常结果，驱动循环 |
| 反馈 | 可选 comment | **必须**提供改进意见 |
| 输出 | `approved_by`, `comment` | `approved`, `feedback` |
| 典型场景 | 生产部署审批 | AI 内容迭代优化 |

#### 3.2.2 YAML 定义

```yaml
- id: review_prompts
  type: review
  name: "审阅分镜头脚本"          # 可选

  # 必填：要审阅的内容（模板表达式，通常引用上一步输出）
  content: "${{ steps.generate_prompts.answer }}"

  # 可选：给用户的提示语
  prompt: "请审阅以上分镜头脚本，是否满意？"

  # 可选：拒绝时是否必须提供反馈（默认 true）
  require_feedback_on_reject: true

  # 可选：超时（默认无超时，一直等待）
  timeout: "30m"

  # 可选：超时策略（默认 continue = 视为通过）
  on_timeout: "continue"    # continue | abort
```

#### 3.2.3 执行行为

1. **渲染 content** — 解析模板表达式，获取要展示的内容
2. **格式化输出** — 将内容以人类可读形式展示到终端
3. **等待用户输入** — 通过 stdin 读取：`y`/`yes` → approved=true；`n`/`no` → 收集反馈
4. **收集反馈** — 多行文本输入，空行结束（仅在 reject 时）
5. **返回结果** — `StepResult.output`:

```json
{
  "approved": true,
  "feedback": null
}
```

或：

```json
{
  "approved": false,
  "feedback": "S02 的画面描述太抽象，需要更具体的视觉元素\nS05 时间太短，建议合并到 S04"
}
```

**关键**：`review` step **永远不会返回错误**。无论 approve 还是 reject，都是 `StepStatus::Success`。reject 是正常的流程控制信号。

#### 3.2.4 终端交互体验

```
═══ [Step: generate_prompts] AI 生成分镜头脚本 (第 1 轮) ═══
    Agent Stream: output:
    {"project":"月夜赏析","video_title":"月夜 - 赏析","shots":[...]}

═══ [Step: review_prompts] 审阅分镜头脚本 (第 1 轮) ═══

  分镜头脚本预览:
  ┌──────────────────────────────────────────────────┐
  │ S01 | 0:00 - 0:30 | 30s | 3 clips               │
  │   月光如流水一般，静静地泻在这一片叶子和花上...      │
  │ S02 | 0:30 - 1:05 | 35s | 4 clips               │
  │   ...                                            │
  └──────────────────────────────────────────────────┘

  请审阅以上分镜头脚本，是否满意？[y/n]: n

  请描述修改意见（空行结束）:
  > S02 的画面描述太抽象，需要更具体的视觉元素
  > 整体色调偏暗，希望加入更多暖色调
  >

  ✓ 反馈已记录，将重新生成分镜头脚本...

═══ [Step: generate_prompts] AI 生成分镜头脚本 (第 2 轮) ═══
    (根据反馈重新生成...)

═══ [Step: review_prompts] 审阅分镜头脚本 (第 2 轮) ═══
  ...
  请审阅以上分镜头脚本，是否满意？[y/n]: y

  ✓ 审阅通过！继续后续步骤...
```

### 3.3 扩展 Loop Executor: `loop.last` 上下文注入

#### 3.3.1 问题

当前 `while` 循环的 condition 在每次迭代**之前**求值，使用的是父上下文。循环体内的子步骤输出不会回传影响条件。

#### 3.3.2 解决方案

在 `execute_while` 中，每次迭代完成后，将本次迭代的子步骤输出以 `loop.last.<step_id>` 形式注入到下次迭代的上下文中。

#### 3.3.3 求值流程

```
迭代 1:
  1. 求值 condition → loop.last 不存在 → null != true → 进入循环
  2. 执行 generate_prompts → 得到 {answer, session_id}
  3. 执行 review_prompts → 得到 {approved: false, feedback: "..."}
  4. 保存 iteration_results 到 loop.last

迭代 2:
  1. 求值 condition → loop.last.review_prompts.approved = false → false != true → 进入循环
  2. generate_prompts 的 agent_input 中使用 loop.last.review_prompts.feedback → 获取反馈
     generate_prompts 的 session_ref 使用 loop.last.generate_prompts.session_id → 复用 session
  3. 执行 review_prompts → 得到 {approved: true, feedback: null}
  4. 保存 iteration_results 到 loop.last

迭代 3 (退出):
  1. 求值 condition → loop.last.review_prompts.approved = true → true != true → false → 退出
```

#### 3.3.4 Rust 实现要点

```rust
async fn execute_while(&self, ...) -> Result<Vec<StepResult>, WorkflowError> {
    let mut results = Vec::new();
    let mut iteration_count = 0u32;
    let mut last_iteration_outputs: HashMap<String, serde_json::Value> = HashMap::new();

    loop {
        // 检查 max_iterations
        if let Some(max) = max_iterations {
            if iteration_count >= max {
                return Err(WorkflowError::MaxIterationsExceeded { max_iterations: max });
            }
        }

        // 创建循环上下文，注入 loop.last
        let mut loop_context = self.create_loop_context(context);
        loop_context.set_variable("loop".to_string(), serde_json::json!({
            "last": last_iteration_outputs,
            "iteration": iteration_count,
        }));

        // 求值条件（在注入了 loop.last 的上下文中）
        let condition_value = self.evaluate_expression(condition, &loop_context)?;
        let should_continue = condition_value.as_bool().unwrap_or(false);

        if !should_continue {
            break;
        }

        // 执行循环体
        let iteration_results = self.execute_loop_body(do_steps, &loop_context).await?;

        // 提取子步骤输出，更新 last_iteration_outputs
        last_iteration_outputs.clear();
        for result in &iteration_results {
            if let Some(output) = &result.output {
                last_iteration_outputs.insert(result.step_id.clone(), output.clone());
            }
        }

        results.extend(iteration_results);
        iteration_count += 1;
    }

    Ok(results)
}
```

**关键改动**：
1. 每次迭代前，将上一次迭代的子步骤输出以 `loop.last.<step_id>` 注入上下文
2. 首次迭代时 `loop.last` 为空对象 `{}`，模板引用得到 `null`
3. 条件求值时 `null != true` → 进入循环（需确认模板引擎 null 比较行为）

### 3.4 Loop 结果输出

循环结束后，后续步骤需要拿到最后一轮 `generate_prompts` 的结果。Loop step 的 output 结构：

```json
{
  "iterations": 2,
  "results": [
    {"step_id": "generate_prompts", "output": {"answer": "...第1轮...", "session_id": "..."}},
    {"step_id": "review_prompts", "output": {"approved": false, "feedback": "..."}},
    {"step_id": "generate_prompts", "output": {"answer": "...第2轮...", "session_id": "..."}},
    {"step_id": "review_prompts", "output": {"approved": true, "feedback": null}}
  ],
  "last_outputs": {
    "generate_prompts": {"answer": "...第2轮...", "session_id": "..."},
    "review_prompts": {"approved": true, "feedback": null}
  }
}
```

后续步骤引用：

```yaml
- id: write_prompts
  tool_args: |
    {
      "path": "${{ steps.setup.prompts_file }}",
      "content": ${{ steps.prompt_loop.last_outputs.generate_prompts.answer | to_json }}
    }
```

需要在 `LoopResponse` 中新增 `last_outputs` 字段。

---

## 4. 方案设计：Agent Session 复用

### 4.1 问题分析

当前 `AgentExecutor` 每次执行都新建 session → 执行 → 销毁。如果用户不满意要重新生成，新建的 agent session 没有之前的对话历史，LLM 看不到自己之前的输出。

### 4.2 方案选型

| 方案 | 描述 | 优点 | 缺点 |
|------|------|------|------|
| **A: Session Pinning** | Agent step 支持指定 `session_ref`，循环体内复用 session | 最自然，历史完全保留，token 效率高 | 需改动 AgentExecutor + LoopExecutor |
| B: History Injection | 每次新建 session，在 `agent_input` 中注入历史对话文本 | 无需引擎改动 | token 浪费，格式不精确 |
| C: Checkpoint History | 序列化 messages 到文件，下次反序列化恢复 | 跨进程也能恢复 | 复杂度高 |

**推荐方案 A**。

### 4.3 ReActAgent: 新增 continue 模式

#### 4.3.1 问题

`run_internal` / `run_internal_stream` 开头都执行 `self.messages.clear()`。

#### 4.3.2 解决

新增 `is_continue` 参数控制是否保留历史：

```rust
// react_agent.rs

impl ReActAgent {
    /// 流式运行（清空历史，首次调用使用）
    pub async fn run_stream(
        &mut self,
        user_input: &str,
        callback: AgentCallback,
    ) -> Result<String, AgentError> {
        self.run_internal_stream(user_input, callback, false).await
    }

    /// 追加式流式运行（保留历史，复用 session 时使用）
    pub async fn continue_run_stream(
        &mut self,
        user_input: &str,
        callback: AgentCallback,
    ) -> Result<String, AgentError> {
        self.run_internal_stream(user_input, callback, true).await
    }
}
```

`run_internal_stream` 改动：

```rust
async fn run_internal_stream(
    &mut self,
    user_input: &str,
    callback: AgentCallback,
    is_continue: bool,       // ← 新增参数
) -> Result<String, AgentError> {

    if is_continue {
        // 追加模式：不清空历史，只追加新的用户消息
        self.messages.push(Message::user(
            format!("<question>{}</question>", user_input)
        ));
    } else {
        // 首次模式：清空历史，重建 system prompt
        self.messages.clear();
        let system_prompt = self.render_system_prompt().await;
        self.messages.push(Message::system(system_prompt));
        self.messages.push(Message::user(
            format!("<question>{}</question>", user_input)
        ));
    }

    // ... 后续 LLM call + tool loop 逻辑不变 ...
}
```

#### 4.3.3 对话历史对比

```
首次调用 run_stream(input="原始文章+音频时长+SRT..."):
  messages after run:
    [0] system: "你必须直接输出结果..."
    [1] user:   "<question>原始文章+音频时长...</question>"
    [2] assistant: "<final_answer>{S01:..., S02:...}</final_answer>"

第2次调用 continue_run_stream(input="用户反馈：S02太抽象"):
  messages after run:
    [0] system: "你必须直接输出结果..."          ← 保留
    [1] user:   "<question>原始文章+音频时长..."  ← 保留
    [2] assistant: "<final_answer>{S01:...}"      ← 保留（第1轮结果）
    [3] user:   "<question>用户反馈：S02太抽象..."  ← 新追加
    [4] assistant: "<final_answer>{S01:..., S02改进...}" ← 新生成
```

LLM 能看到完整上下文：原始需求 + 自己上一次的输出 + 用户修改意见。

### 4.4 AgentManager: 新增 continue 方法

```rust
// react_agent.rs — AgentManager 新增方法

impl AgentManager {
    /// 追加式同步运行（保留历史）
    pub async fn continue_sync(
        &self,
        session_id: &str,
        user_input: &str,
        callback: Option<AgentCallback>,
    ) -> Result<String, AgentError> {
        let mut sessions = self.sessions.write().await;
        let agent = sessions.get_mut(session_id)
            .ok_or_else(|| AgentError::SessionNotFound(session_id.to_string()))?;

        if let Some(cb) = callback {
            agent.continue_run_with_callback(user_input, cb).await
        } else {
            agent.continue_run(user_input).await
        }
    }

    /// 追加式流式运行（保留历史）
    pub async fn continue_sync_stream(
        &self,
        session_id: &str,
        user_input: &str,
        callback: AgentCallback,
    ) -> Result<String, AgentError> {
        let mut sessions = self.sessions.write().await;
        let agent = sessions.get_mut(session_id)
            .ok_or_else(|| AgentError::SessionNotFound(session_id.to_string()))?;
        agent.continue_run_stream(user_input, callback).await
    }
}
```

### 4.5 StepDefinition: 新增 session 控制字段

```rust
// types.rs

pub struct StepDefinition {
    // ... 现有字段 ...

    // Agent 步骤：session 引用
    // 如果指定，复用该 session（continue 模式）；如果不指定，新建 session
    #[serde(default)]
    pub session_ref: Option<String>,   // 模板表达式，如 "${{ loop.last.generate_prompts.session_id }}"

    /// Session 生命周期控制
    /// - None / "step": step 结束后销毁（默认行为，向后兼容）
    /// - "manual": 不自动销毁，由外部管理（loop 或 workflow 层面）
    #[serde(default)]
    pub session_scope: Option<String>,
}
```

### 4.6 AgentExecutor: 支持 session 复用

```rust
// agent_executor.rs — 改造后的 execute 核心逻辑

impl Executor for AgentExecutor {
    async fn execute(
        &self,
        step: &StepDefinition,
        context: &ExecutionContext,
    ) -> Result<StepResult, WorkflowError> {
        let started_at = Utc::now();
        let step_id = &step.id;

        let llm_config = parse_llm_provider_config(step, context)?;
        let llm = create_llm_provider(&llm_config)?;
        let input = resolve_agent_input(step, context)?;
        let use_stream = step.agent_stream.unwrap_or(false);

        // ──── 解析 session_ref ────
        let session_ref = step.session_ref.as_ref()
            .map(|expr| resolve_template(expr, context))
            .transpose()?
            .filter(|s| !s.is_empty());  // 空字符串视为 None

        // ──── 决定新建还是复用 ────
        let (session_id, is_new_session) = if let Some(existing_id) = session_ref {
            // 复用已有 session
            if self.agent_manager.session_exists(&existing_id).await {
                tracing::info!("[AgentExecutor] Reusing session: {}", existing_id);
                (existing_id, false)
            } else {
                // session 不存在（可能 loop.last 无数据），fallback 到新建
                tracing::warn!("[AgentExecutor] session_ref '{}' not found, creating new", existing_id);
                let id = self.agent_manager
                    .create_session_with_llm(llm, step.agent_system_prompt.as_deref())
                    .await?;
                (id, true)
            }
        } else {
            // 新建 session
            let id = self.agent_manager
                .create_session_with_llm(llm, step.agent_system_prompt.as_deref())
                .await?;

            // 注册工具（仅新建时）
            if let Some(tool_defs) = &step.agent_tools {
                for tool_def in tool_defs {
                    let handler = create_tool_handler(tool_def, &self.builtin_registry)?;
                    let descriptor = ToolDescriptor {
                        name: tool_def.name.clone(),
                        description: tool_def.description.clone().unwrap_or_default(),
                        json_schema: tool_def.json_schema.clone(),
                        handler,
                    };
                    self.agent_manager.register_tool(&id, descriptor).await?;
                }
            }

            (id, true)
        };

        // 设置 max_iterations
        if let Some(max_iter) = step.agent_max_iterations {
            self.agent_manager.set_max_iterations(&session_id, max_iter).await?;
        }

        // ──── 执行推理 ────
        let result = if is_new_session {
            if use_stream {
                let callback = build_stream_callback(step_id.clone());
                self.agent_manager.run_sync_stream(&session_id, &input, callback).await?
            } else {
                self.agent_manager.run_sync(&session_id, &input, None).await?
            }
        } else {
            // 复用 session：追加模式，保留历史
            if use_stream {
                let callback = build_stream_callback(step_id.clone());
                self.agent_manager.continue_sync_stream(&session_id, &input, callback).await?
            } else {
                self.agent_manager.continue_sync(&session_id, &input, None).await?
            }
        };

        // ──── Session 销毁决策 ────
        let scope = step.session_scope.as_deref().unwrap_or("step");
        match scope {
            "manual" => {
                tracing::info!("[AgentExecutor] Session {} kept alive (manual scope)", session_id);
            }
            _ => {
                // "step" 或默认：step 结束后销毁（仅限非 session_ref 引用的 session）
                if session_ref.is_none() {
                    self.agent_manager.destroy_session(&session_id).await;
                }
                // session_ref 引用的 session 由创建者负责销毁
            }
        }

        let completed_at = Utc::now();
        let duration_ms = (completed_at - started_at).num_milliseconds() as u64;

        Ok(StepResult::success(
            step_id.clone(),
            serde_json::json!({
                "answer": result,
                "session_id": session_id,    // 输出 session_id，供后续迭代引用
            }),
        ).with_timing(started_at, duration_ms))
    }
}
```

### 4.7 Loop 结束时清理 Session

```rust
// loop.rs — execute_while 循环结束后

// 清理所有迭代中创建的 agent session（session_scope=manual 的）
for result in &results {
    if let Some(output) = &result.output {
        if let Some(session_id) = output.get("session_id").and_then(|v| v.as_str()) {
            if self.agent_manager.session_exists(session_id).await {
                self.agent_manager.destroy_session(session_id).await;
                tracing::info!("[LoopExecutor] Cleaned up agent session: {}", session_id);
            }
        }
    }
}
```

### 4.8 完整交互时序

```
═══════════════════════════════════════════════════
 迭代 1
═══════════════════════════════════════════════════

[generate_prompts]
  session_ref = default('') → ""
  → effective_session_ref = None
  → 新建 session "agent-a1b2c3"
  → 注册 tools
  → run_sync_stream(原始文章+SRT+风格+...):
      messages = [
        system("你必须直接输出结果..."),
        user("<question>原始文章+音频时长+SRT+风格+首次无修改意见...</question>"),
      ]
      → LLM 推理
      → assistant("<final_answer>{shots:[S01,S02,...]}</final_answer>")
  → session_scope="manual" → 不销毁
  → 输出: {answer: "{shots:...}", session_id: "agent-a1b2c3"}

[review_prompts]
  → 展示分镜头脚本
  → 用户输入: n
  → 反馈: "S02 太抽象，需要更多暖色调"
  → 输出: {approved: false, feedback: "S02 太抽象..."}

═══════════════════════════════════════════════════
 迭代 2
═══════════════════════════════════════════════════

[generate_prompts]
  session_ref = loop.last.generate_prompts.session_id = "agent-a1b2c3"
  → session 存在 → 复用
  → continue_sync_stream(用户反馈):
      messages = [
        system("你必须直接输出结果..."),             ← 保留
        user("<question>原始文章+音频时长+...</question>"), ← 保留
        assistant("<final_answer>{S01,S02,...}</final_answer>"), ← 保留（第1轮结果）
        user("<question>用户反馈：S02 太抽象，需要更多暖色调...</question>"), ← 新追加
      ]
      → LLM 看到 完整上下文 + 自己之前的输出 + 用户反馈
      → 生成改进后的分镜脚本
      → assistant("<final_answer>{S01,S02改进,...}</final_answer>")
  → session_ref 有值 → 不销毁
  → 输出: {answer: "{shots:改进后...}", session_id: "agent-a1b2c3"}

[review_prompts]
  → 展示改进后的分镜头脚本
  → 用户输入: y
  → 输出: {approved: true, feedback: null}

═══════════════════════════════════════════════════
 循环结束
═══════════════════════════════════════════════════

[LoopExecutor]
  → condition: loop.last.review_prompts.approved = true → true != true → false → 退出
  → 清理: 从 results 中提取 session_id="agent-a1b2c3" → destroy_session()
  → 输出: {iterations: 2, last_outputs: {...}}
```

### 4.9 Token 开销对比

| 方案 | 每轮 token 消耗 | 3 轮总消耗 |
|------|----------------|-----------|
| 当前（每次新建，历史丢弃） | ~4K prompt + ~2K completion | ~18K |
| 历史注入（prompt 里拼历史文本） | ~4K + 历史×轮次 + ~2K | ~30K+ |
| **Session 复用（本方案）** | **首次 ~4K，后续 ~4K + 历史，completion ~2K** | **~22K** |

Session 复用比「历史注入」省在：不需要在 prompt 中重复原始文章/SRT 等大段内容（它们已经在 messages 历史中），LLM 直接从上下文窗口中读取。而且 messages 格式比手工拼文本更精确（保留 tool call/observation 等结构）。

---

## 5. 改动清单

### 5.1 Review Loop 相关

| # | 文件 | 改动 | 复杂度 |
|---|------|------|--------|
| 1 | `src/core/types.rs` | `StepType` 枚举加 `Review`；`StepDefinition` 加 `content`, `prompt`, `require_feedback_on_reject` | 低 |
| 2 | `src/executors/review.rs` | 新建文件：ReviewExecutor，从 stdin 读取用户决策和反馈 | 中 |
| 3 | `src/executors/mod.rs` | 在 executor dispatch 中加入 `Review` 分支 | 低 |
| 4 | `src/executors/loop.rs` | `execute_while` 中注入 `loop.last` 上下文；`LoopResponse` 增加 `last_outputs` | 中 |
| 5 | `src/core/template.rs` | 确认 `null != true` 的行为，可能需要调整 `default` filter | 低 |
| 6 | `src/core/parser.rs` | 解析 `review` 类型步骤的新字段 | 低 |

### 5.2 Agent Session 复用相关

| # | 文件 | 改动 | 复杂度 |
|---|------|------|--------|
| 7 | `src/agent/react_agent.rs` | `ReActAgent` 新增 `continue_run` / `continue_run_stream`，`run_internal_*` 接收 `is_continue` 参数 | 中 |
| 8 | `src/agent/react_agent.rs` | `AgentManager` 新增 `continue_sync` / `continue_sync_stream` 方法 | 低 |
| 9 | `src/core/types.rs` | `StepDefinition` 新增 `session_ref` 和 `session_scope` | 低 |
| 10 | `src/executors/agent_executor.rs` | `execute` 中根据 `session_ref` 决定新建/复用，根据 `session_scope` 决定是否销毁，输出中包含 `session_id` | 中 |
| 11 | `src/executors/loop.rs` | 循环结束后清理 agent session（从 results 中提取 session_id 并 destroy） | 低 |

---

## 6. 完整 YAML 示例

以下是 doc-to-video.yaml 中 Phase 3-4 部分改造后的完整写法：

```yaml
  # ═══════════════════════════════════════════
  # Phase 3: 分镜头脚本迭代优化
  # ═══════════════════════════════════════════

  - id: get_duration
    name: "获取音频时长"
    type: shell
    depends_on: [tts]
    run: |
      DURATION=$(ffprobe -v error -show_entries format=duration -of csv=p=0 ${{ steps.setup.audio_path }})
      echo "$DURATION"

  - id: read_md
    name: "读取清洗后文本"
    type: tool
    depends_on: [clean_text]
    tool_name: read_file
    tool_args: '{"path": "${{ steps.setup.cleaned_path }}"}'

  - id: read_srt
    name: "读取 edge-tts SRT"
    type: tool
    depends_on: [tts]
    tool_name: read_file
    tool_args: '{"path": "${{ steps.setup.srt_path }}"}'

  - id: prompt_loop
    name: "分镜头脚本迭代优化"
    type: loop
    loop:
      while:
        condition: "${{ loop.last.review_prompts.approved | default(false) != true }}"
        max_iterations: 10
    do_steps:
      - id: generate_prompts
        name: "AI 生成分镜头脚本"
        type: agent
        agent_stream: true
        # 首次迭代 session_ref 为空 → 新建 session
        # 后续迭代引用上一轮的 session_id → 复用 session（保留对话历史）
        session_ref: "${{ loop.last.generate_prompts.session_id | default('') }}"
        session_scope: "manual"
        agent_input: |
          ## 原始文章

          ${{ steps.read_md }}

          ## 音频总时长

          ${{ steps.get_duration.stdout }} 秒

          ## SRT 字幕文件（含精确时间戳和文本段落）

          ${{ steps.read_srt }}

          ## 视觉风格参数

          ${{ inputs.style }}

          ## 分镜头数量限制

          最多 ${{ inputs.max_shots }} 个镜头。

          ## 用户修改意见（如有）

          ${{ loop.last.review_prompts.feedback | default('（首次生成，无修改意见）') }}

          请根据以上信息生成分镜头脚本 JSON。直接输出 JSON，不要任何解释。
        agent_system_prompt: |
          你必须直接输出结果。

          ## 任务

          根据中文文章内容、SRT 字幕时间戳和视觉风格，生成 prompts.json。

          ## 输出格式（严格遵守）
          最终输出以下格式：
          final_answer xml标签包裹如下json
          {"project":"...","video_title": "","video_summary": "","total_duration_sec":...,"style_guide":"...","default_params":{"model_id":"qwen","resolution":"1080p","duration":5,"fps":24,"aspect_ratio":"16:9"},"shots":[{"id":"S01","timestamp":"0:00 - 0:30","duration_sec":30,"clips_needed":3,"prompt":"Chinese visual description","negative_prompt":"elements to exclude"}]}
          图片+音频合成的视频，后续发表到视频平台，video_title用于视频标题，video_summary用于视频简介

          ## 分镜规则

          1. 以 SRT 的文本段落和时间戳为基础，合并语义相近的连续字幕为一个镜头
          2. 每个镜头的 timestamp 必须与 SRT 中的时间戳对齐
          3. duration_sec = 该镜头覆盖的 SRT 时间段长度
          4. 所有 duration_sec 之和 = 音频总时长
          5. clips_needed = ceil(duration_sec / 10)，最少 1
          6. prompt（200-400 汉字）必须包含：画面内容、构图、色调、氛围、镜头运动
          7. negative_prompt 排除不想要的元素
          8. timestamp 格式 M:SS - M:SS，连续覆盖全部时长
          9. ID 从 S01 递增
          10. shots 总数不得超过 max_shots 限制

          ## 风格映射

          | style 参数 | 画风 |
          |-----------|------|
          | ink_wash | 中国水墨山水风格 |
          | modern | 美式漫画/波普风格 |
          | sketch | 铅笔素描质感 |

        agent_max_iterations: 10

      - id: review_prompts
        name: "审阅分镜头脚本"
        type: review
        depends_on: [generate_prompts]
        content: "${{ steps.generate_prompts.answer }}"
        prompt: "请审阅以上分镜头脚本，是否满意？"
        require_feedback_on_reject: true
        timeout: "30m"

  # ═══════════════════════════════════════════
  # Phase 4: 写入最终 prompts + 后续流程
  # ═══════════════════════════════════════════

  - id: write_prompts
    name: "写入 prompts.json"
    type: tool
    depends_on: [prompt_loop]
    tool_name: write_file
    tool_args: |
      {
        "path": "${{ steps.setup.prompts_file }}",
        "content": ${{ steps.prompt_loop.last_outputs.generate_prompts.answer | to_json }}
      }

  - id: gen_images
    name: "千问 AI 生图"
    type: shell
    depends_on: [write_prompts]
    run: |
      cd ${{ steps.setup.video_dir }}
      DASHSCOPE_API_KEY=${{ inputs.dashscope_api_key }} \
      python3 ${{ steps.setup.scripts_dir }}/generate_images.py --skip-existing
    timeout: "30m"

  # ... assemble, burn_subs, pack 等后续步骤不变 ...
```

---

## 7. 风险与边界情况

| 风险 | 应对 |
|------|------|
| 用户一直不满意，无限循环 | `max_iterations: 10` 硬上限 |
| `review` 超时 | `on_timeout: continue` 默认视为通过，避免卡死 |
| stdin 非交互模式（CI/CD） | 检测 stdin 是否 TTY，非 TTY 时自动批准 |
| `loop.last` 首次为空 | `default(false)` filter 兜底 |
| 模板引擎中 null 比较 | 测试并统一模板引擎的 null 语义 |
| Agent session 内存泄漏 | Loop 结束时统一清理所有 manual scope 的 session |
| 超长对话历史导致 token 超限 | 循环次数限制（10 次）+ 可选的 `clear_history` 策略 |
| `session_ref` 引用了已被 destroy 的 session | `session_exists()` 检查 + fallback 到新建 |

---

## 附录：MVP 优先级

**Phase 1（最小可行）**：

1. `ReviewExecutor`（stdin 交互）
2. `execute_while` 的 `loop.last` 注入
3. `ReActAgent` 的 `continue_run` 模式
4. `AgentExecutor` 的 `session_ref` 支持

这四个改动就能让整个流程跑起来。

**Phase 2（体验优化）**：

- 交互式 TUI（`dialoguer` crate 的选择界面）
- `last_outputs` 模板引用
- Web UI 审阅（WebSocket 推送到浏览器）

**Phase 3（运维保障）**：

- 审阅历史记录（每轮的 prompts + feedback 持久化到文件）
- Session 超时自动清理
- Token 用量统计与报告
