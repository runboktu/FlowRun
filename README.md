[中文](./README-zh.md) | English

# flow-run

A declarative workflow engine designed for AI Agents. Define workflows via YAML, with support for HTTP requests, Shell commands, conditional branching, loops, sub-workflow composition, manual approval, and more.

## Installation

```bash
cargo build --release
# Binary located at target/release/flow-run
```

## Command Overview

```
flow-run [OPTIONS] <WORKFLOW_FILE> <COMMAND>

Commands:
  run         Execute a workflow
  resume      Resume workflow execution from a checkpoint
  validate    Validate a workflow definition
  dry-run     Simulate workflow execution (show execution plan without running)
  checkpoint  Checkpoint management
  history     View execution history
  schema      Output JSON Schema for workflow definitions

Options:
  -v, --verbose          Enable verbose logging
  -C, --config <CONFIG>  Specify a configuration file
```

## Subcommand Details

### run — Execute Workflow

Parse the YAML workflow file, build a DAG scheduling graph, and execute all steps in dependency order.

```bash
flow-run <workflow.yaml> run [OPTIONS]
```

**Parameters:**

| Parameter | Short | Description |
|:---|:---|:---|
| `--input <key=value>` | `-i` | Pass input parameters to the workflow (can be used multiple times) |
| `--json` | | Output complete execution result in JSON format |
| `--dry-run` | | Simulate execution — parse and display the execution plan only |
| `--normal` | | Normal execution mode (default) |
| `--async-mode` | | Asynchronous execution mode |
| `--daemon` | | Daemon mode |

**Examples:**

```bash
# Execute an HTTP workflow
flow-run examples/01_basic_http.yaml run \
  --input api_url=https://jsonplaceholder.typicode.com

# Execute a Shell workflow with multiple parameters
flow-run examples/02_basic_shell.yaml run \
  --input project_name=myapp \
  --input environment=production

# JSON output (suitable for programmatic parsing)
flow-run examples/01_basic_http.yaml run \
  --input api_url=https://jsonplaceholder.typicode.com \
  --json

# Simulate execution (does not actually run steps)
flow-run examples/02_basic_shell.yaml run --dry-run \
  --input project_name=myapp

# Enable verbose logging
flow-run -v examples/01_basic_http.yaml run \
  --input api_url=https://jsonplaceholder.typicode.com
```

**Output Example (human-readable):**

```
Result: Success
Step Results:
  [OK] fetch_user
  [OK] display_user  Username: Bret, Email: Sincere@april.biz

Metrics:
  Total steps: 2 | Success: 2 | Failed: 0 | Skipped: 0
  Duration: 1835ms

Workflow Outputs:
  user_name: "Bret"
  user_email: "Sincere@april.biz"
```

**Exit Codes:**
- `0` — Workflow executed successfully
- `1` — Workflow execution failed or YAML parse error

---

### dry-run — Simulate Execution

Parse the workflow file, compute the DAG topological sort, and display a complete execution analysis report without actually executing any steps.

```bash
flow-run <workflow.yaml> dry-run [OPTIONS]
```

**Parameters:**

| Parameter | Short | Description |
|:---|:---|:---|
| `--input <key=value>` | `-i` | Pass input parameters (for display only) |
| `--json` | | Output execution plan in JSON format |

**Example:**

```bash
flow-run examples/11_comprehensive_cicd.yaml dry-run
```

**Output Contents:**

- Workflow basic info (name, version, description, step count, cycle dependency check)
- Global configuration (timeout, failure strategy, checkpoint, max concurrency, retry strategy)
- Input parameter definitions (name, type, required) and actual passed values
- Workflow output definitions and template expressions
- Step list (type, dependencies, timeout, retry configuration)
  - HTTP steps: display API URL and method
  - Shell steps: display command preview
  - Parallel steps: display sub-steps and max concurrency
  - Loop steps: display loop configuration
  - Condition steps: display condition expression and branch count
  - Workflow steps: display sub-workflow path
  - Approve steps: display approver list
- DAG structure (node count, edge count, all dependency relations `A ──→ B`)
- Topological sort execution plan (batch list, parallel markers, step outgoing edges)

**Output Example (CI/CD workflow, key sections):**

```
══════════════════════════════════════════════
  Dry Run: CI/CD Complete Pipeline
══════════════════════════════════════════════
  Description: A complete CI/CD workflow
  Version: 1.0.0
  Steps: 10
  DAG Check: No cycle dependencies

── Global Configuration ──
  Timeout: 30m
  Failure Strategy: Pause
  Max Concurrency: 4

── Topological Sort (Execution Plan) ──
  8 batches total
  Batch 1: 1 step
    ├─ checkout (Shell) - Check out code [out→ detect_changes]
  Batch 2: 1 step
    ├─ detect_changes (Shell) - Detect changes [out→ build_frontend, build_backend]
  Batch 3: (parallel) 2 steps
    ├─ build_frontend (Shell) - Build frontend [out→ test_parallel, security_scan]
    ├─ build_backend (Shell) - Build backend [out→ test_parallel, security_scan]
  ...

── DAG Structure ──
  Nodes: 10 | Edges: 12
  checkout ──→ detect_changes
  detect_changes ──→ build_frontend
  detect_changes ──→ build_backend
  ...
```

---

### resume — Resume from Checkpoint

Load a specified checkpoint and resume workflow execution. Useful when a workflow fails midway (`on_failure: pause`) — fix the issue and continue from the failure point.

```bash
flow-run <workflow.yaml> resume --checkpoint-id <ID> [OPTIONS]
```

**Parameters:**

| Parameter | Short | Description |
|:---|:---|:---|
| `--checkpoint-id <ID>` | | Checkpoint ID to resume from (required) |
| `--input <key=value>` | `-i` | Override input parameters |
| `--json` | | Output result in JSON format |

**Checkpoint Directory:** Resume operations look for checkpoint files under `/tmp/flow-run-checkpoints`.

**Example:**

```bash
# Resume from a specific checkpoint
flow-run examples/12_checkpoint_resume.yaml resume \
  --checkpoint-id cp_abc123
```

---

### validate — Validate Workflow Definition

Check YAML syntax correctness and detect DAG cycle dependencies.

```bash
flow-run <workflow.yaml> validate [OPTIONS]
```

**Parameters:**

| Parameter | Description |
|:---|:---|
| `--show-dag` | Display step list and DAG structure |
| `--json` | Output workflow definition in JSON format |

**Examples:**

```bash
# Validate workflow
flow-run examples/11_comprehensive_cicd.yaml validate

# Validate and show DAG structure
flow-run examples/11_comprehensive_cicd.yaml validate --show-dag

# Output complete JSON definition
flow-run examples/11_comprehensive_cicd.yaml validate --json
```

---

### checkpoint — Checkpoint Management

Manage checkpoints saved during workflow execution.

```bash
flow-run <workflow.yaml> checkpoint <ACTION>
```

**Subcommands:**

| Subcommand | Description |
|:---|:---|
| `list` | List all checkpoints |
| `show <ID>` | Display checkpoint details |
| `clean` | Clean up checkpoints |

**list Parameters:**

```bash
flow-run <workflow.yaml> checkpoint list [OPTIONS]
# --verbose, -v    Show detailed information
# --status <STATUS> Filter by status
# --json           JSON format output
```

**show Parameters:**

```bash
flow-run <workflow.yaml> checkpoint show <CHECKPOINT_ID> [OPTIONS]
# --steps, -s      Show step details
# --json           JSON format output
```

**clean Subcommand:**

```bash
# Clean by ID
flow-run <workflow.yaml> checkpoint clean id <ID1> <ID2> ...

# Clean all (requires confirmation)
flow-run <workflow.yaml> checkpoint clean all --confirm

# Clean checkpoints older than N days
flow-run <workflow.yaml> checkpoint clean older-than --days 7

# Clean by status
flow-run <workflow.yaml> checkpoint clean status <STATUS>

# Keep only the most recent N
flow-run <workflow.yaml> checkpoint clean keep --count 5
```

---

### history — View Execution History

```bash
flow-run <workflow.yaml> history [OPTIONS]
# --limit, -l <N>    Maximum number of entries to display (default 20)
# --status <STATUS>  Filter by status
# --failed           Show only failed executions
# --json             JSON format output
```

---

### schema — Output JSON Schema

Output the JSON Schema for workflow definitions, useful for editor autocompletion and validation.

```bash
flow-run <workflow.yaml> schema [OPTIONS]
# --output, -o <PATH>  Write to file
# --pretty              Pretty-print output
```

**Examples:**

```bash
# Output to terminal
flow-run examples/01_basic_http.yaml schema --pretty

# Write to file (for editor use)
flow-run examples/01_basic_http.yaml schema -o workflow-schema.json
```

---

## Workflow YAML Syntax

### Basic Structure

```yaml
name: "Workflow Name"
description: "Workflow Description"
version: "1.0.0"

inputs:
  - name: api_url
    type: string
    required: true

steps:
  - id: step_id
    name: "Step Name"
    type: http          # http / shell / parallel / loop / condition / workflow / approve
    # ... step configuration

outputs:
  result_key: "${{ steps.step_id.output.path }}"
```

### Step Types

| Type | Description | Key Configuration |
|:---|:---|:---|
| `http` | HTTP request | `api`, `method`, `headers`, `body` |
| `shell` | Shell command | `run`, `env`, `safe_mode` |
| `parallel` | Parallel execution | `steps`, `max_concurrent` |
| `loop` | Loop execution | `loop`, `do_steps` |
| `condition` | Conditional branch | `expression`, `then_steps`, `else_steps` |
| `workflow` | Sub-workflow | `workflow`, `inputs`, `error_strategy` |
| `approve` | Manual approval | `message`, `approvers`, `auto_approve_on` |

### Template Expressions

```yaml
# Variable references
${{ inputs.variable_name }}
${{ steps.step_id.output_name }}
${{ variables.custom_var }}

# Path access
${{ steps.fetch.response.body.data }}
${{ steps.fetch.response.body.items[0].name }}

# Filter chains
${{ steps.fetch.response.body.name | uppercase }}
${{ steps.fetch.response.body.name | truncate(10) }}
${{ variables.items | join(', ') }}

# Conditional expressions
${{ inputs.env || 'development' }}
```

### Built-in Filters

| Filter | Description | Example |
|:---|:---|:---|
| `uppercase` | Convert to uppercase | `hello` → `HELLO` |
| `lowercase` | Convert to lowercase | `HELLO` → `hello` |
| `trim` | Strip whitespace | ` hello ` → `hello` |
| `default(v)` | Default value | `null` → `v` |
| `length` | Length | `[1,2,3]` → `3` |
| `slice(s,e)` | Slice | `[1,2,3] \| slice(0,2)` → `[1,2]` |
| `first` | First element | `[1,2,3]` → `1` |
| `last` | Last element | `[1,2,3]` → `3` |
| `join(sep)` | Join | `[a,b] \| join('-')` → `a-b` |
| `split(sep)` | Split | `a-b \| split('-')` → `[a,b]` |
| `replace(o,n)` | Replace | `hello \| replace(l,L)` → `heLLo` |
| `truncate(n)` | Truncate | `longtext \| truncate(5)` → `long...` |
| `to_json` | To JSON | `{a:1}` → `'{"a":1}'` |
| `from_json` | Parse JSON | `'{"a":1}'` → `{a:1}` |

---

## Example Workflows

Complete examples are in the `examples/` directory:

```bash
# HTTP request
flow-run examples/01_basic_http.yaml run \
  --input api_url=https://jsonplaceholder.typicode.com

# Shell command
flow-run examples/02_basic_shell.yaml run \
  --input project_name=myapp --input environment=staging

# Step dependencies
flow-run examples/03_basic_dependencies.yaml run

# Parallel execution
flow-run examples/04_intermediate_parallel.yaml run

# Retry strategy
flow-run examples/05_intermediate_retry.yaml run

# Template expressions
flow-run examples/06_intermediate_templates.yaml run

# Loop execution
flow-run examples/07_advanced_loop.yaml run

# Conditional branching
flow-run examples/08_advanced_condition.yaml run

# Sub-workflow
flow-run examples/09_advanced_subworkflow.yaml run

# Manual approval
flow-run examples/10_advanced_approval.yaml run

# CI/CD pipeline
flow-run examples/11_comprehensive_cicd.yaml run

# Checkpoint save and resume
flow-run examples/12_checkpoint_resume.yaml run
```

For Rust code examples, see [`examples/README.md`](examples/README.md).

## Environment Variables

```bash
# Control log level
RUST_LOG=debug flow-run examples/01_basic_http.yaml run
RUST_LOG=flow_run=trace flow-run examples/01_basic_http.yaml run
```
