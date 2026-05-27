# Claude Code — manual setup

Claude Code supports both plugin-based and direct hook installation. Either works.

## Option A: Plugin

Inside Claude Code:

```
/plugin marketplace add NishantJoshi00/claude-plugins
/plugin install sidekick@nishant-plugins
```

This registers the bundled `PreToolUse`, `PostToolUse`, and `UserPromptSubmit` hooks for you.

## Option B: Direct hooks in `~/.claude/settings.json`

```json
{
  "hooks": {
    "PreToolUse": [
      { "matcher": "MultiEdit|Edit|Write", "hooks": [{ "type": "command", "command": "sidekick hook" }] }
    ],
    "PostToolUse": [
      { "matcher": "MultiEdit|Edit|Write", "hooks": [{ "type": "command", "command": "sidekick hook" }] }
    ],
    "UserPromptSubmit": [
      { "matcher": "", "hooks": [{ "type": "command", "command": "sidekick hook" }] }
    ]
  }
}
```

The `UserPromptSubmit` entry is optional — it is the one that adds your Neovim visual selection to Claude's prompt context. Drop it if you only want buffer protection and refresh.

## Verify

```bash
sidekick doctor
```

Look for the `Claude Code hook registered` row.
