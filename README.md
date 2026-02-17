# Grotto 🪸

Multi-agent orchestration for [OpenClaw](https://github.com/openclaw/openclaw). Give your agent a team.

## Install

Tell your OpenClaw agent:

> Read https://raw.githubusercontent.com/jlgrimes/grotto/master/skill/SKILL.md and install grotto as a skill.

Or manually:

```bash
# Install the CLI
git clone https://github.com/jlgrimes/grotto.git /tmp/grotto
cd /tmp/grotto && cargo install --path crates/grotto-cli
rm -rf /tmp/grotto

# Install the OpenClaw skill
cp -r skill/ ~/.openclaw/workspace/skills/grotto/
```

## What it does

Your OpenClaw agent spawns Claude Code sessions in tmux panes that work on tasks in parallel. The agent acts as team lead — spawning, steering, and monitoring a team of coding agents.

```
┌─────────────────────────────────────┐
│  Your OpenClaw Agent (Team Lead)    │
└──────┬──────────┬──────────┬────────┘
       │          │          │
  ┌────▼───┐ ┌───▼────┐ ┌───▼────┐
  │Agent 1 │ │Agent 2 │ │Agent 3 │  ← Claude Code in tmux
  └────────┘ └────────┘ └────────┘
```

## Requirements

- [OpenClaw](https://github.com/openclaw/openclaw)
- [Claude Code](https://docs.anthropic.com/en/docs/claude-code) CLI
- [tmux](https://github.com/tmux/tmux)
- Rust toolchain

## License

MIT
