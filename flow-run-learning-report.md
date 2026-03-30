[中文](./flow-run-learning-report-zh.md) | English

# flow-run Learning Guide

## 1. Project Overview

**flow-run** is a **declarative workflow engine** written in Rust, designed specifically for AI Agents. It defines workflows via YAML, uses a DAG (Directed Acyclic Graph) scheduling engine to automatically resolve step dependencies, executes independent steps in parallel, and provides checkpoint-based resume, conditional branching, loops, template expressions, and more.

**Core Problems Solved:**
- Lack of orchestration when Agents execute multi-step tasks
- Need to retry from scratch after failure, wasting completed work
- Inability to execute independent steps in parallel
- Lack of conditional branching and loop support

**Tech Stack:** Rust 2021 Edition / Tokio (async runtime) / Clap (CLI) / Serde (serialization) / reqwest (HTTP)

---

## 2. Project Structure Overview

```
flow-run/
├── Cargo.toml                    # Rust project configuration and dependencies
├── flow-run-design.md            # Detailed design document (highly recommended to read first)
│
├── src/
│   ├── main.rs                   # CLI entry point, command dispatch
│   ├── lib.rs                    # Library entry, exports 4 sub-modules
│   │
│   ├── core/                     # Core engine (YAML parsing → DAG scheduling → Execution)
│   │   ├── types.rs              # All type definitions (~767 lines, most important file)
│   │   ├── parser.rs             # YAML parsing + validation (uniqueness, dependencies, cycle detection)
│   │   ├── dag.rs                # DAG scheduler + Scheduler execution engine (~896 lines)
│   │   ├── context.rs            # Execution context (inputs/outputs/variables/state tracking)
│   │   └── template.rs           # Template expression engine (${{...}} syntax + filters)
│   │
│   ├── executors/                # Step executors
│   │   ├── mod.rs                # Executor trait definition
│   │   ├── http.rs               # HTTP request executor
│   │   ├── shell.rs              # Shell command executor
│   │   ├── loop.rs               # Loop executor
│   │   ├── condition.rs          # Conditional branch executor
│   │   ├── workflow.rs           # Sub-workflow executor (includes WorkflowRunner trait)
│   │   └── approve.rs            # Manual approval executor
│   │
│   ├── cli/                      # Command-line interface
│   │   ├── mod.rs
│   │   └── commands.rs           # Clap definitions for all subcommands (run/resume/validate/...)
│   │
│   └── utils/                    # Utility modules
│       ├── mod.rs
│       ├── error.rs              # Unified error types (category codes: A/B/C/D/E/F/G)
│       ├── retry.rs              # Retry engine (fixed/exponential/Fibonacci backoff + jitter)
│       └── checkpoint.rs         # Checkpoint management (save/load/timeout context)
│
└── examples/                     # YAML workflow examples (11, from basic to comprehensive)
    ├── 01_basic_http.yaml        # HTTP request
    ├── 02_basic_shell.yaml       # Shell command
    ├── 03_basic_dependencies.yaml # Step dependencies
    ├── 04_intermediate_parallel.yaml  # Parallel execution
    ├── 05_intermediate_retry.yaml     # Retry
    ├── 06_intermediate_templates.yaml # Template expressions
    ├── 07_advanced_loop.yaml          # Loop
    ├── 08_advanced_condition.yaml     # Conditional branch
    ├── 09_advanced_subworkflow.yaml   # Sub-workflow
    ├── 10_advanced_approval.yaml      # Manual approval
    ├── 11_comprehensive_cicd.yaml     # Comprehensive CI/CD
    └── code/                           # Rust code examples (7)
```

---

## 3. Core Concepts Overview

### 3.1 YAML Workflow Definition

A flow-run workflow is a YAML file with the following top-level fields:

```yaml
name: deploy-application          # Workflow name
description: Automated deployment workflow  # Description (optional)
version: "1.0"                    # Version (optional)

config:                           # Global configuration
  timeout: 300s                   #   Total timeout
  retry:                          #   Global retry strategy
    max_attempts: 3
    strategy: exponential
  on_failure: pause               #   Failure strategy: abort / pause / continue
  checkpoint: /tmp/deploy.state   #   Checkpoint path
  max_concurrent: 5               #   Max concurrency

inputs:                           # Input parameter definitions
  - name: app_name
    type: string
    required: true
  - name: environment
    type: string
    default: staging
    enum: [staging, production]

outputs:                          # Output definitions
  deployment_id: ${{steps.deploy.response.body.id}}

steps:                            # Step list
  - id: fetch_data
    type: http
    api: https://api.example.com/data
    method: GET
```

### 3.2 Seven Step Types

| Type | Purpose | Key Fields |
|:---|:---|:---|
| `http` | HTTP API calls | `api`, `method`, `headers`, `body`, `cache` |
| `shell` | Execute Shell commands | `run`, `env`, `safe_mode`, `allowed_commands` |
| `parallel` | Execute sub-steps in parallel | `steps`, `max_concurrent`, `rate_limit` |
| `loop` | Loop execution | `loop` (forEach/while/range), `do_steps` |
| `condition` | Conditional branching | `expression`, `then_steps`, `else_steps` |
| `workflow` | Sub-workflow | `workflow` (path), `inputs`, `error_strategy` |
| `approve` | Manual approval | `message`, `approvers`, `auto_approve_on` |

### 3.4 Dependencies and Parallelism

Steps declare dependencies via `depends_on`. The DAG scheduler automatically performs topological sorting, executing steps in batches — **steps within the same batch that have no dependencies on each other execute in parallel**.

```yaml
steps:
  - id: fetch_data          # Batch 1
    type: http
    api: https://api.example.com/data

  - id: prepare_env         # Batch 1 (parallel with fetch_data)
    type: shell
    run: "mkdir -p /tmp/output"

  - id: process_data        # Batch 2 (depends on fetch_data)
    type: shell
    run: "cat /tmp/data | jq"
    depends_on: [fetch_data]

  - id: save_result         # Batch 3 (depends on process_data and prepare_env)
    type: shell
    run: "cp /tmp/data /output"
    depends_on: [process_data, prepare_env]
```

### 3.4 Template Expressions

Use `${{...}}` syntax to reference variables, step outputs, and apply filter chains:

```yaml
# Variable reference
api: ${{ inputs.api_url }}

# Step output reference (nested path + array index)
item: ${{ steps.fetch.outputs.data.items[0].name }}

# Filter chain (pipe-separated)
message: ${{ steps.check.outputs.result | uppercase | truncate(50) }}

# Default value
fallback: ${{ steps.optional.value | default("unknown") }}

# Conditional expression
env: ${{ inputs.environment || "staging" }}
```

**Built-in filters:** `uppercase`, `lowercase`, `capitalize`, `trim`, `default(val)`, `to_json`, `from_json`, `length`, `slice(s,e)`, `first`, `last`, `join(sep)`, `split(sep)`, `replace(old,new)`, `regex_extract(pat)`, `truncate(n)`, `base64_encode`, `base64_decode`, `format_timestamp`, `format_duration`

---

## 4. Architecture and Execution Flow

### 4.1 Overall Pipeline

```
YAML File → Parser → Validator → DAG Scheduler → Executor(s) → Result
               │         │           │               │
               │         │           │               ├─ HTTP Executor
               │         │           │               ├─ Shell Executor
               │         │           │               ├─ Parallel Executor
               │         │           │               ├─ Loop Executor
               │         │           │               ├─ Condition Executor
               │         │           │               ├─ Workflow Executor
               │         │           │               └─ Approve Executor
               │         │           │
               │         │           └─ Checkpoint management (saved after each batch)
               │         │
               │         └─ Cycle dependency detection (DFS)
               └─ serde_yaml deserialization
```

### 4.2 Key Data Flow

1. **Parser** (`core/parser.rs`) parses YAML into a `WorkflowDefinition` struct
2. **DagScheduler** (`core/dag.rs`) builds adjacency list and in-degree table from `WorkflowDefinition`
3. **topological_sort** runs Kahn's algorithm, splitting steps into batches (`Vec<Vec<StepId>>`)
4. **Scheduler::run** executes by batch; within each batch, steps run in parallel via `tokio::spawn`, with concurrency controlled by `Semaphore`
5. Each step's result is written to `ExecutionContext`, allowing subsequent steps to reference previous step outputs via the template engine

### 4.3 Core Struct Relationships

```
WorkflowDefinition (top-level definition)
├── config: WorkflowConfig
├── inputs: Vec<InputDefinition>
├── outputs: HashMap<String, String>
├── steps: Vec<StepDefinition>     ←── Each step can contain sub-steps
│   ├── id, name, type
│   ├── depends_on: Vec<String>
│   ├── steps (parallel sub-steps)
│   ├── then_steps / else_steps (condition)
│   ├── do_steps (loop body)
│   └── workflow (sub-workflow path)
├── on: HooksConfig
└── trigger: Vec<TriggerConfig>

ExecutionContext (runtime state)
├── inputs: HashMap<String, Value>
├── step_outputs: HashMap<StepId, StepResult>  ←── Core of inter-step data passing
├── completed_steps: HashSet<StepId>
├── failed_steps: HashSet<StepId>
└── variables: HashMap<String, Value>

StepResult (single step execution result)
├── step_id, status, started_at, completed_at, duration_ms
├── output: Option<Value>           ←── Referenced by subsequent steps via templates
└── error: Option<StepError>
```

---

## 5. Core Module Details

### 5.1 Type System (`core/types.rs`)

This is the most important file in the entire project (~767 lines), defining all data structures. Key types:

| Type | Description |
|:---|:---|
| `WorkflowDefinition` | Top-level workflow definition |
| `StepDefinition` | Step definition (a large struct containing fields for all step types) |
| `StepType` | Step type enum: Http/Shell/Parallel/Loop/Condition/Workflow/Approve |
| `WorkflowConfig` | Global configuration (timeout, retry, concurrency, checkpoint, resume strategy, etc.) |
| `StepResult` | Step execution result (provides `success()`/`failed()`/`skipped()` constructors) |
| `WorkflowResult` | Workflow final result (status, metrics, step result list, outputs, errors) |
| `LoopConfig` | Loop configuration (ForEach/While/Range modes) |
| `OnFailureStrategy` | Failure strategy: Abort / Pause (save checkpoint) / Continue |

`StepDefinition` uses a flat struct to accommodate all step types — each type only uses its relevant fields, with others set to `None`. This is a common Rust pattern that trades more fields for avoiding nested enum complexity.

### 5.2 YAML Parsing and Validation (`core/parser.rs`)

`WorkflowParser` provides 3 core methods:

```rust
// Load from file
WorkflowParser::from_file("workflow.yaml")?;
// Parse from string
WorkflowParser::from_str(yaml_content)?;
// Validate workflow definition
WorkflowParser::validate(&workflow)?;
```

Validation includes three steps:
1. **Step ID uniqueness check** — Recursively checks all sub-steps (parallel steps, condition then/else_steps, loop do_steps)
2. **Dependency validity** — Ensures each `depends_on` reference points to an existing step ID
3. **Cycle dependency detection** — Uses DFS to detect back edges in the recursion stack

### 5.3 DAG Scheduler (`core/dag.rs`)

**DagScheduler** is responsible for building the dependency graph and performing topological sorting:
- Maintains `adjacency` (adjacency list) and `in_degree` (in-degree table)
- `topological_sort()` returns `Vec<Vec<StepId>>`, the batched execution plan
- Uses Kahn's algorithm (BFS level-order), where each level's nodes form a batch

**Scheduler** is the actual execution engine:
- `run()` — Execute workflow from the beginning
- `resume()` — Resume execution from a checkpoint
- `execute_batch()` — Parallel execution via `tokio::spawn` with `Semaphore` concurrency control
- `execute_step()` — Dispatch to different execution methods based on step type

### 5.4 Execution Context (`core/context.rs`)

`ExecutionContext` is the runtime "shared state store":
- Stores all input parameters, step outputs, and variables
- Provides `evaluate()` method to evaluate `${{...}}` expressions
- Provides `resolve_path()` method to resolve dot-separated paths (e.g., `steps.deploy.response.body.data[0]`)
- Tracks step completion/failure status

### 5.5 Template Expression Engine (`core/template.rs`)

`TemplateEngine` processes `${{...}}` template expressions, supporting:
- **Path access**: `inputs.api_url`, `steps.fetch.response.body.data[0].name`
- **Filter chains**: `value | uppercase | truncate(10)`
- **Conditional expressions**: `inputs.env || "staging"`, `inputs.count == 10`
- **18 built-in filters**

Implementation highlights:
- Uses `Regex` to match `${{...}}` patterns
- `find_operator()` method intelligently identifies `||` and `==` operators, ignoring same-named characters inside quotes and parentheses
- `navigate_path()` returns `Value::Null` instead of erroring, working with `default` and `||` usage

### 5.6 Step Executors (`executors/`)

Each executor implements the `Executor` trait (except `WorkflowExecutor` which uses the `WorkflowRunner` trait):

| Executor | File | Responsibility |
|:---|:---|:---|
| HTTP Executor | `executors/http.rs` | Build request, send, parse response, validate expectations |
| Shell Executor | `executors/shell.rs` | Execute `sh -c`, inject environment variables, safety mode check |
| Loop Executor | `executors/loop.rs` | ForEach/While/Range loop modes |
| Condition Executor | `executors/condition.rs` | Evaluate expression, execute then/else branches |
| Workflow Executor | `executors/workflow.rs` | Load sub-workflow, prepare inputs, context isolation |
| Approve Executor | `executors/approve.rs` | Manual approval (send notifications, poll results, handle timeout) |

### 5.7 Retry Engine (`utils/retry.rs`)

`RetryEngine` provides automatic retry with backoff strategies:
- Three backoff strategies: Fixed, Exponential (`delay = initial * factor^attempt`), Fibonacci
- Jitter to avoid thundering herd: multiplies computed delay by a random factor of 0.8~1.2
- Configurable retryable HTTP status codes (default: 408/429/500/502/503/504) and error types
- Maximum delay cap (default: 30s)

### 5.8 Checkpoint System (`utils/checkpoint.rs`)

Checkpoints enable resume from breakpoint:
- `Checkpoint` struct saves complete execution state (completed steps, failed steps, step outputs, variables, current batch, timeout context)
- `CheckpointManager` provides save (JSON file), load, list, and delete operations
- `TimeoutContext` tracks workflow-level and step-level timeout consumption, allowing resumption to inherit remaining timeout

### 5.9 Error System (`utils/error.rs`)

Error types use letter-coded categories for easy Agent parsing:

| Prefix | Category | Examples |
|:---|:---|:---|
| A | Workflow errors | A001 file not found, A004 cycle dependency |
| B | Execution errors | B001 HTTP failure, B003 timeout |
| C | Checkpoint errors | C001 checkpoint not found |
| D | Template errors | D002 variable undefined, D005 filter not found |
| E | Approval errors | E001 approval rejected |
| F | Hook errors | F001 hook timeout |
| G | Trigger errors | G001 Webhook signature invalid |

---

## 6. CLI Usage

```
flow-run <WORKFLOW_FILE> [OPTIONS] <SUBCOMMAND>

Subcommands:
  run         Execute workflow
  resume      Resume from checkpoint
  validate    Validate workflow definition
  dry-run     Simulate execution
  checkpoint  Checkpoint management (list/show/clean)
  history     View execution history
  schema      Output JSON Schema
```

```bash
# Execute workflow
flow-run workflow.yaml run --input key=value --json

# Validate workflow (show DAG structure)
flow-run workflow.yaml validate --show-dag

# Dry run
flow-run workflow.yaml dry-run

# Resume from checkpoint
flow-run workflow.yaml resume --checkpoint_id cp_xxx

# List checkpoints
flow-run workflow.yaml checkpoint list --verbose
```

---

## 7. Recommended Learning Path

### Phase 1: Understand the Design (read design.md)

Start with `flow-run-design.md`, which contains complete architecture diagrams, data structure designs, and pseudocode examples. This is the fastest way to build a holistic understanding.

### Phase 2: Run the Examples

Run the YAML examples in `examples/` in order:

```bash
# Basic
cargo run -- examples/01_basic_http.yaml validate
cargo run -- examples/02_basic_shell.yaml validate
cargo run -- examples/03_basic_dependencies.yaml validate

# Intermediate
cargo run -- examples/04_intermediate_parallel.yaml validate
cargo run -- examples/05_intermediate_retry.yaml validate
cargo run -- examples/06_intermediate_templates.yaml validate

# Advanced
cargo run -- examples/07_advanced_loop.yaml validate
cargo run -- examples/08_advanced_condition.yaml validate
cargo run -- examples/09_advanced_subworkflow.yaml validate
cargo run -- examples/10_advanced_approval.yaml validate

# Comprehensive
cargo run -- examples/11_comprehensive_cicd.yaml validate
```

### Phase 3: Read Core Source Code (in dependency order)

1. **`src/core/types.rs`** — All type definitions, understand the data model
2. **`src/utils/error.rs`** — Error system, understand error categorization
3. **`src/core/parser.rs`** — YAML parsing and validation logic
4. **`src/core/context.rs`** — Execution context and expression evaluation
5. **`src/core/template.rs`** — Template engine and filter system
6. **`src/core/dag.rs`** — DAG scheduler + Scheduler execution engine
7. **`src/utils/retry.rs`** — Retry engine
8. **`src/utils/checkpoint.rs`** — Checkpoint system
9. **`src/executors/*.rs`** — Various step executors
10. **`src/cli/commands.rs`** — CLI command definitions
11. **`src/main.rs`** — Entry function, connecting all modules

### Phase 4: Review Rust Code Examples

`examples/code/` contains 7 Rust code examples demonstrating how to use flow-run as a library:
- `01_load_workflow` — Load a workflow
- `02_execution_context` — Execution context
- `03_dag_scheduler` — DAG scheduling
- `04_template_engine` — Template engine
- `05_retry_engine` — Retry engine
- `06_checkpoint` — Checkpoint
- `07_full_execution` — Full execution

### Phase 5: Run Tests

```bash
cargo test          # Run all tests
cargo test -- --nocapture  # Show test output
```

---

## 8. Key Design Decision Analysis

### 8.1 Why Rust?

- AI Agents need non-interactive, structured-output tools
- Rust provides memory safety, zero-cost async (tokio), and single-binary deployment
- Native JSON output, no `--json` flag needed

### 8.2 Why Flat Struct Instead of Nested Enums?

`StepDefinition` is a large struct containing fields for all step types. This simplifies YAML deserialization — Serde can directly map YAML to the struct without complex tag parsing. The tradeoff is more fields (~30), but type safety is maintained through `Option<T>`.

### 8.3 How Does Checkpointing Enable Resume?

After each batch completes, the Scheduler serializes the current state to JSON. On resume, it loads the checkpoint, skips completed batches, and continues from the next batch. `TimeoutContext` preserves elapsed time so resumption inherits remaining timeout rather than restarting the clock.

### 8.4 Why Does the Template Engine Return Null Instead of Erroring?

When a path doesn't exist (e.g., `inputs.missing_field`), the template engine returns `Value::Null` instead of erroring. This allows `default` filters and `||` operators to gracefully handle missing values — an Agent-friendly design that avoids interrupting entire workflows due to non-critical field absence.

---

## 9. Common YAML Pattern Reference

### HTTP Request + Result Reference
```yaml
- id: fetch
  type: http
  api: ${{ inputs.api_url }}/users
  method: GET
- id: process
  type: shell
  run: echo ${{ steps.fetch.response.body.name }}
  depends_on: [fetch]
```

### Parallel Execution
```yaml
- id: parallel_tasks
  type: parallel
  max_concurrent: 10
  rate_limit:
    requests_per_second: 5
    burst: 10
  steps:
    - id: task_1
      type: http
      api: https://api.example.com/1
    - id: task_2
      type: http
      api: https://api.example.com/2
```

### Conditional Branching
```yaml
- id: deploy
  type: condition
  expression: inputs.environment == 'production'
  then_steps:
    - id: prod_deploy
      type: shell
      run: ./deploy-prod.sh
  else_steps:
    - id: dev_deploy
      type: shell
      run: ./deploy-dev.sh
```

### Loop
```yaml
- id: process_items
  type: loop
  loop:
    for_each:
      over: ${{ steps.fetch.outputs.data.items }}
      as: item
  do_steps:
    - id: process
      type: shell
      run: echo "Processing ${{ variables.item.name }}"
```

### Sub-workflow
```yaml
- id: run_tests
  type: workflow
  workflow: ./test-suite.yaml
  inputs:
    test_env: ${{ inputs.environment }}
  error_strategy: continue
  timeout: 120s
```

### Manual Approval
```yaml
- id: approve_deploy
  type: approve
  message: "Confirm deployment of ${{ inputs.version}} to production?"
  approvers: [team-leads@company.com]
  timeout: 3600s
  auto_approve_on:
    - condition: "${{ inputs.environment == 'staging' }}"
      reason: "Staging auto-approved"



## Code Detailed Explanation

This code is part of the `DagScheduler::new` method, used to **initialize the DAG (Directed Acyclic Graph) data structures**:

```rust
for step_id in &step_ids {
    adjacency.insert(step_id.clone(), Vec::new());  // Adjacency list
    in_degree.insert(step_id.clone(), 0);           // In-degree table
}
```
# DAG Detailed Analysis
## Two Core Data Structures

### 1. `adjacency: HashMap<StepId, Vec<StepId>>` - Adjacency List

```rust
adjacency.insert(step_id.clone(), Vec::new());
```

- **Meaning**: Records "which subsequent steps can be reached from the current step"
- **Initialization**: Each step is initialized with an empty `Vec`
- **Example**: If `step1 -> step2`, then `adjacency["step1"] = ["step2"]`

### 2. `in_degree: HashMap<StepId, usize>` - In-degree Table

```rust
in_degree.insert(step_id.clone(), 0);
```

- **Meaning**: Records "how many prerequisite steps depend on the current step"
- **Initialization**: Each step's in-degree is initialized to `0`
- **Example**: If `step1 -> step2`, then `in_degree["step2"] = 1`

## Complete Flow Illustration

Assume the following dependency relationships:
```
step1 → step2 → step4
    ↘ step3 ↗
```

### After Initialization:

| step_id | adjacency | in_degree |
|---------|-----------|-----------|
| step1 | `[]` | 0 |
| step2 | `[]` | 0 |
| step3 | `[]` | 0 |
| step4 | `[]` | 0 |

### After Processing `depends_on`:

```rust
for step in &steps {
    if let Some(deps) = &step.depends_on {
        for dep in deps {
            // dep -> step.id edge
            adjacency.entry(dep.clone()).or_default().push(step.id.clone());
            *in_degree.entry(step.id.clone()).or_insert(0) += 1;
        }
    }
}
```

| step_id | adjacency | in_degree |
|---------|-----------|-----------|
| step1 | `[step2, step3]` | 0 |
| step2 | `[step4]` | 1 |
| step3 | `[step4]` | 1 |
| step4 | `[]` | 2 |



## Why Are These Two Data Structures Needed?

These are at the core of **topological sorting (Kahn's algorithm)**:

1. Steps with **in_degree = 0** have no prerequisites and can execute immediately
2. After executing a step, decrement the `in_degree` of its neighbors in `adjacency` by 1
3. When a step's `in_degree` becomes 0, add it to the execution queue

```rust
// Usage in topological_sort
for (step_id, &deg) in &in_degree {
    if deg == 0 {
        queue.push_back(step_id.clone());  // Can execute immediately
    }
}
```

## Summary

The purpose of this code is to:
1. **Collect all step IDs**
2. **Initialize the adjacency list and in-degree table** (each step is isolated, in-degree = 0)
3. Subsequent code populates these two tables based on `depends_on`, building the complete dependency graph

This explanation is added as comments.
```

Yes, this is a **manually implemented topological sort**, using the classic **Kahn's algorithm** (BFS version).

## Algorithm Flow

```rust
pub fn topological_sort(&self) -> Result<Vec<Vec<StepId>>, WorkflowError> {
    // 1. First check for cycle dependencies
    if self.has_cycle()? {
        return Err(WorkflowError::CycleDetected);
    }

    // 2. Initialize: copy in-degree table, find all steps with in-degree 0
    let mut in_degree = self.in_degree.clone();
    let mut queue: VecDeque<StepId> = VecDeque::new();
    let mut batches: Vec<Vec<StepId>> = Vec::new();

    for (step_id, &deg) in &in_degree {
        if deg == 0 {
            queue.push_back(step_id.clone());
        }
    }

    // 3. Execute topological sort (Kahn's algorithm core loop)
    while !queue.is_empty() {
        // Take out current batch (all in-degree 0 steps can execute in parallel)
        let current_batch: Vec<StepId> = queue.drain(..).collect();

        // Decrement in-degree for subsequent steps of the current batch
        for step_id in &current_batch {
            if let Some(neighbors) = self.adjacency.get(step_id) {
                for neighbor in neighbors {
                    if let Some(deg) = in_degree.get_mut(neighbor) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push_back(neighbor.clone());
                        }
                    }
                }
            }
        }

        batches.push(current_batch);
    }

    Ok(batches)
}
```

## Key Characteristics

This implementation returns **batched topological sorting** `Vec<Vec<StepId>>`, not just a flat sequence:

| Batch | Included Steps | Description |
|-------|---------------|-------------|
| `batches[0]` | Steps with in-degree 0 | Can execute in parallel immediately |
| `batches[1]` | Steps depending on first batch completion | |
| ... | ... | |

## Example

```
step1 → step2 → step4
    ↘ step3 ↗
```

Topological sort result: `[["step1"], ["step2", "step3"], ["step4"]]`

- Batch 1: `step1` (no dependencies, execute immediately)
- Batch 2: `step2`, `step3` (execute in parallel, both depend on `step1`)
- Batch 3: `step4` (depends on `step2` and `step3`)

## Why Manual Implementation?

1. **Batched execution**: Need to support parallel execution of steps at the same level
2. **Cycle detection**: Check for cycle dependencies before sorting
3. **Workflow engine integration**: Return result is directly used by `Scheduler::run()` for batch execution

---

## 10. Inter-Step Result Passing Mechanism in Detail

This is flow-run's most critical runtime mechanism — how one step's execution result is referenced by subsequent steps.

### 10.1 Overall Data Flow

```
Step A completes execution
       │
       ▼
StepResult { step_id: "A", output: Some({...}), status, ... }
       │
       ▼  In Scheduler.run(), written after each batch executes
       │
ExecutionContext.step_outputs["A"] = StepResult(...)
       │
       ▼  Before Step B executes, build template context
       │
template_ctx = {
    "inputs":   { "api_url": "https://..." },
    "steps":    { "A": <StepResult.output value> },  ← Key: only the output field
    "variables": { ... }
}
       │
       ▼  TemplateEngine parses ${{ steps.A.xxx }} in B's run/api field
       │
${{ steps.A.response.body.title }}  →  Navigate JSON path layer by layer
       │
Step B gets Step A's output data
```

### 10.2 StepResult.output JSON Structure

Different step types have different `output` field structures:

**Shell step** — raw stdout output:
```json
{ "stdout": "Directory created\nConfiguration file written\n" }
```

**HTTP step** — wrapped in response structure:
```json
{
  "response": {
    "status_code": 200,
    "body": { "id": 1, "title": "...", "userId": 1 }
  }
}
```

**Parallel step** — array of sub-step outputs:
```json
{
  "results": [
    { "response": { "status_code": 200, "body": {...} } },
    { "response": { "status_code": 200, "body": {...} } },
    { "response": { "status_code": 200, "body": {...} } }
  ]
}
```

**Workflow (sub-workflow) step** — includes status and outputs:
```json
{
  "workflow": "examples/sub.yaml",
  "status": "Success",
  "outputs": { "artifact_path": "/tmp/build/app.tar.gz" },
  "metrics": { "total_steps": 3, "success_steps": 3, ... }
}
```

### 10.3 Write: When Results Are Stored in Context

In `Scheduler::run()` (`src/core/dag.rs:215-218`), after all steps in each batch complete, results are immediately written to the shared context:

```rust
for result in &batch_results {
    let mut ctx = self.context.write().await;
    ctx.step_outputs.insert(result.step_id.clone(), result.clone());
}
```

`step_outputs` is `HashMap<StepId, StepResult>`, with step ID as the key and the complete `StepResult` (including status, output, error, duration_ms, etc.) as the value.

**Write timing matters**: Within the same batch, parallel steps that complete later overwrite earlier ones (typically no conflict since IDs differ). But across batches, ordering is strict — batch N results are fully written before batch N+1 starts executing.

### 10.4 Read: How Template Context Is Built

When Step B is about to execute (`execute_shell_step` or `execute_http_step`), the template context is built from `ExecutionContext` (`src/core/dag.rs:490-503`):

```rust
// Build steps context: only include output field, for direct template access
let mut steps_ctx = serde_json::Map::new();
for (step_id, result) in &ctx.step_outputs {
    if let Some(output) = &result.output {
        steps_ctx.insert(step_id.clone(), output.clone());
    }
}
template_ctx.insert("steps".to_string(), serde_json::Value::Object(steps_ctx));
```

Key design:
- Only takes `result.output`, excluding `status`, `error`, `duration_ms`, and other metadata
- If a step failed and `output` is `None`, that step ID won't appear in the template context
- Template context also includes `inputs` (input parameters) and `variables` (workflow variables + loop variables)

### 10.5 Resolution: How `${{ steps.A.x.y }}` Is Parsed

When Step B's `run` field contains `${{ steps.fetch_data.response.body.title }}`:

1. **Regex extraction**: `TemplateEngine` uses `\$\{\{([^}]+)\}\}` to extract the inner expression `steps.fetch_data.response.body.title`

2. **Operator detection** (priority from high to low):
   - `||` default value operator: `inputs.env || "staging"` → returns right side if left is Null/empty string
   - `==` equality comparison: `inputs.risk_level == 'high'` → returns `true`/`false`
   - `|` filter chain: `value | uppercase | truncate(10)` → applies filters sequentially

3. **Path resolution**: Split by `.` into `["steps", "fetch_data", "response", "body", "title"]`
   - `resolve_path()` first gets root key `steps` from context → gets the steps_ctx object
   - `navigate_path()` navigates layer by layer:
     - `steps_ctx["fetch_data"]` → HTTP step's output: `{"response": {"status_code": 200, "body": {...}}}`
     - `["response"]` → `{"status_code": 200, "body": {...}}`
     - `["body"]` → `{"id": 1, "title": "..."}`
     - `["title"]` → `"..."`

4. **Array index**: `variables.items[0].name`
   - `items[0]` is split by `[`, taking field `items` (empty string means direct array access), then indexing by number

5. **Missing paths return Null**: When a path doesn't exist, returns `Value::Null` instead of erroring. This allows `||` and `default()` filters to gracefully handle missing values.

### 10.6 Differences Between the Two Path Resolution Systems

| Feature | `TemplateEngine.resolve_path()` | `ExecutionContext.resolve_path()` |
|:---|:---|:---|
| File location | `src/core/template.rs` | `src/core/context.rs` |
| When path doesn't exist | Returns `Value::Null` (friendly) | Returns `WorkflowError::PathNotFound` (error) |
| Use case | Step executors parsing `run`/`api` templates | `evaluate()` method, output parsing |
| Array out of bounds | Returns `Value::Null` | Returns `PathNotFound` |
| Filter support | Yes | No |

### 10.7 Special Cases

**Passing between sub-steps of Parallel steps**: In `execute_parallel_step()` (`src/core/dag.rs:550-553`), each sub-step's result is **immediately** written to context after execution:

```rust
{
    let mut ctx = context.write().await;
    ctx.step_outputs.insert(sub_step.id.clone(), result.clone());
}
```

This means later sub-steps within a parallel group can reference outputs of earlier-completed sub-steps in the same group.

**Variable passing in Loop steps**: Loop variables are passed through `context.variables["loop"]`, and are extracted to the top level when building the template context:

```rust
if let Some(loop_vars) = ctx.variables.get("loop") {
    template_ctx.insert("loop".to_string(), loop_vars.clone());
}
```

So within a loop body, you can access the current iteration variable via `${{ variables.loop.current }}` or `${{ loop.current }}`.

**Sub-workflow context isolation**: Sub-workflows create independent `ExecutionContext` instances, by default passing through the parent workflow's inputs (configurable via `passthrough_vars` for specific variables, or `isolation: true` for complete isolation). Sub-workflow outputs are returned to the parent workflow in a wrapped structure.

### 10.8 Complete Example: HTTP → Shell Result Passing

```yaml
steps:
  - id: fetch
    type: http
    api: https://api.example.com/users/1
    method: GET
    # output = {"response": {"status_code": 200, "body": {"name": "Alice", "email": "a@b.com"}}}

  - id: display
    type: shell
    depends_on: [fetch]
    # Template resolution chain:
    #   steps.fetch → get fetch step's output
    #   .response → {"status_code": 200, "body": {...}}
    #   .body → {"name": "Alice", "email": "a@b.com"}
    #   .name → "Alice"
    run: echo "Username: ${{ steps.fetch.response.body.name }}"
```
