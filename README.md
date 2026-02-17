# Grotto 🪸

**Claude Code teammates. OpenClaw steering the ship.**

Spawn a team of [Claude Code](https://docs.anthropic.com/en/docs/claude-code) agents in tmux that work in parallel — coordinated by your [OpenClaw](https://github.com/openclaw/openclaw) agent as team lead.

## Install

Tell your OpenClaw agent:

> Read https://raw.githubusercontent.com/jlgrimes/grotto/master/skill/SKILL.md and install grotto as a skill.

Or manually:

```bash
git clone https://github.com/jlgrimes/grotto.git /tmp/grotto
cd /tmp/grotto && cargo install --path crates/grotto-cli
rm -rf /tmp/grotto

# Install the OpenClaw skill
cp -r skill/ ~/.openclaw/workspace/skills/grotto/
```

## How it works

```
┌─────────────────────────────────────┐
│  Your OpenClaw Agent (Team Lead)    │
└──────┬──────────┬──────────┬────────┘
       │          │          │
  ┌────▼───┐ ┌───▼────┐ ┌───▼────┐
  │Agent 1 │ │Agent 2 │ │Agent 3 │  ← Claude Code in tmux
  └────────┘ └────────┘ └────────┘
```

Your OpenClaw agent spawns Claude Code sessions as tmux panes. They self-organize via a shared task board, communicate with each other, and ship code in parallel. The lead steers in real-time — no fire-and-forget.

## Requirements

- [OpenClaw](https://github.com/openclaw/openclaw)
- [Claude Code](https://docs.anthropic.com/en/docs/claude-code) CLI
- [tmux](https://github.com/tmux/tmux)
- Rust toolchain

## License

MIT
