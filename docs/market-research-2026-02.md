# Grotto market research (2026-02)

## Scope and framing
This memo focuses on adjacent tools in multi-agent coding/orchestration and identifies practical opportunities for **Grotto’s tmux-first, local orchestration model**.

---

## 1) Competitor snapshot (positioning + strengths/weaknesses)

### OpenHands (open-source cloud coding agents)
**Positioning**
- Open, model-agnostic coding agent platform with secure sandbox execution and broad integrations (GitHub/GitLab/Slack/CI).
- Optimized for autonomous task execution at scale (from single task to many parallel runs).

**Strengths**
- Strong enterprise narrative: self-hosting, auditability, access control.
- Good model/provider flexibility.
- Clear benchmark/measurement push (OpenHands Index around ability/cost/runtime tradeoffs).

**Weaknesses / gaps**
- Cloud/runtime heavy; less natural for local-first terminal-native workflows.
- More platform complexity than many teams need for day-1 adoption.
- Observability is strong at platform level, weaker as “developer co-pilot in my existing tmux muscle memory.”

### MetaGPT (multi-agent “AI software company”)
**Positioning**
- Role-based multi-agent framework (PM/architect/engineer patterns), SOP-driven software lifecycle generation.

**Strengths**
- Very strong ideation/planning artifact generation from sparse prompts.
- Clear role abstractions and process framing.

**Weaknesses / gaps**
- Heavier on prescribed process than on practical local engineering loop.
- Can over-index on document generation vs repo-grounded execution quality.
- Less compelling for teams wanting minimal ceremony and direct terminal control.

### AutoGen / Microsoft Agent Framework direction
**Positioning**
- Developer framework for building agentic apps; now increasingly folded toward enterprise-grade, event-driven orchestration stacks.

**Strengths**
- Strong abstractions for complex orchestration and extensibility.
- Enterprise-grade trajectory (state, telemetry, middleware, integrations).

**Weaknesses / gaps**
- Framework complexity and integration overhead.
- Better for building agent products than immediately shipping code in an existing repo with minimal setup.

### CrewAI
**Positioning**
- Python-first multi-agent framework (Crews + Flows) moving toward enterprise automation control plane.

**Strengths**
- Good role/task abstractions and workflow control.
- Strong commercialization and community momentum.

**Weaknesses / gaps**
- Requires Python framework adoption and orchestration design effort.
- “Production” still often means adding extra systems around it.
- Less native to terminal-first local codebase workflows.

### Devin (Cognition)
**Positioning**
- Managed AI software engineer product for autonomous coding tasks and backlog throughput.

**Strengths**
- Strong value for repetitive, bounded engineering tasks at scale.
- Parallelization and managed UX can unlock clear throughput gains.

**Weaknesses / gaps**
- Managed/closed platform tradeoffs (cost, control, local sovereignty).
- Even by own framing, strongest on clear/junior-style bounded tasks; less reliable on ambiguous/socially complex work.
- Lower fit for teams that require local-only execution and explicit, inspectable orchestration.

### Claude Code agent teams / workflows (native)
**Positioning**
- First-party multi-session/team coordination in Claude Code.

**Strengths**
- Native to Claude Code users; low incremental friction.
- Built-in parallel teammate model with direct coordination patterns.

**Weaknesses / gaps**
- Experimental status + known limitations.
- Product scope prioritizes Claude Code UX, not cross-tool orchestration layer.
- Potential gap for users who want a stable, tmux-persistent, model/tool-agnostic operations layer.

---

## 2) Unmet user jobs (where existing options are weak)

1. **“I need reliable local parallelism without platform overhead.”**
   - Users want to split work across agents in minutes, not deploy infra.

2. **“I need operational visibility in terminal-native form.”**
   - Not only dashboards; users want pane-level logs, replay, and deterministic event trails tied to git actions.

3. **“I need safe autonomy with explicit guardrails, not black-box runs.”**
   - Approval points for risky actions, policy-by-default, and auditable command boundaries.

4. **“I need repeatable team patterns for common coding missions.”**
   - Bug triage swarm, migration swarm, test hardening swarm, release-readiness swarm.

5. **“I need orchestration that survives interruptions and is resumable.”**
   - Crash/reboot/session-drop recovery with clean continuation.

6. **“I need cost/performance control at task-routing level.”**
   - Route easy tasks to cheaper models and hard tasks to stronger models with measured outcomes.

---

## 3) Grotto differentiation thesis

### Core thesis
**Grotto should own “Local Agent Ops for software teams”: the fastest path from task → parallel agent execution → reviewed merge, using tmux-native control and inspectable file/event state.**

### Why this can win
- **Local-first trust**: code stays near developer environment; no mandatory SaaS control plane.
- **tmux-native ergonomics**: meets advanced developers in their existing workflow instead of replacing it.
- **Operational clarity**: event log + per-agent state makes runs inspectable/debuggable.
- **Composable architecture**: can integrate Claude Code now while staying model/tool agnostic over time.

### Anti-thesis (what to avoid)
- Competing head-on as a “fully managed autonomous engineer.”
- Building heavy enterprise platform surface area before nailing local reliability and day-1 productivity.

---

## 4) 30/60-day feature bets (implementation-oriented)

## 30-day bets

### Small bet: Mission templates + one-command launch
**What**
- Add built-in templates for common parallel workflows:
  - `grotto spawn --template bugfix-swarm`
  - `grotto spawn --template test-hardening`
  - `grotto spawn --template migration-slice`
- Templates predefine roles, kickoff prompts, and completion criteria.

**Why**
- Shrinks orchestration prompt-crafting overhead.
- Makes Grotto value obvious to first-time users.

**Implementation notes**
- Template TOML/JSON in repo + render to initial `tasks.md` + agent-specific starter prompts.
- Keep user override flags for agent count, model, and constraints.

### Medium bet: Guardrail policies and approval gates
**What**
- Policy file in `.grotto/config.toml` for risky actions:
  - write scope restrictions
  - command allow/deny patterns
  - require approval for destructive commands or dependency upgrades
- Add CLI approval queue (`grotto approvals`).

**Why**
- Increases trust and production-readiness for local autonomy.

**Implementation notes**
- Event type additions: `policy_blocked`, `approval_requested`, `approval_granted`.
- Enforce at orchestration boundary before command dispatch.

### Larger bet: Deterministic run replay + resume
**What**
- “Resume session exactly from event stream + agent checkpoints.”
- `grotto resume <session-id>` reconstructs status and restarts interrupted agents.

**Why**
- Directly solves a top operational pain in multi-agent local workflows.

**Implementation notes**
- Persist minimal checkpoint metadata per agent.
- Add replay loader for `events.jsonl` + integrity markers.

## 60-day bets

### Small-medium bet: Outcome scorecard per session
**What**
- End-of-run metrics report: lead time, task completion %, test pass delta, PR readiness, token/cost per completed task.

**Why**
- Lets users tune orchestration strategy and model routing based on evidence.

### Medium bet: Git-aware merge pipeline
**What**
- Structured branch strategy per agent, conflict detection, and optional auto-stacked PR prep.

**Why**
- Converts “agents did work” into “team can merge safely” with less manual stitching.

### Larger bet: Multi-model routing plugin
**What**
- Route tasks by complexity/risk profile to configured models/providers.

**Why**
- Strong differentiation vs single-model-first workflows; measurable cost-performance gains.

---

## 5) Measurable success metrics

## Product adoption metrics
- **Time to first parallel run (TTFPR)**: median < 10 minutes from install.
- **Week-1 repeat usage**: % of users running >=3 sessions in first week.
- **Template attach rate**: % sessions started with template.

## Execution quality metrics
- **Session completion rate**: sessions ending with all assigned tasks resolved.
- **Human intervention rate**: manual steer/approval actions per session (target down over time, not zero).
- **Resume success rate**: interrupted sessions successfully resumed.

## Engineering outcome metrics
- **PR-ready output rate**: sessions producing mergeable branches/PRs.
- **Cycle time reduction**: baseline vs Grotto-assisted completion time for repeated task classes.
- **Cost efficiency**: cost per completed task by template/model route.

---

## Recommended near-term strategy
1. **Prioritize reliability + guardrails over new autonomy depth** (trust compounds faster than novelty).
2. **Productize repeatable mission templates** to turn Grotto into “parallel coding playbooks” rather than generic orchestration.
3. **Make outcomes measurable by default** so teams can justify ongoing usage with throughput + quality evidence.

---

## Sources consulted (2026-02)
- OpenHands site + OpenHands Index blog
- MetaGPT GitHub/README
- Microsoft AutoGen GitHub + Microsoft Research AutoGen page
- CrewAI GitHub/docs positioning
- Devin docs + Cognition performance review blog
- Claude Code docs on agent teams
