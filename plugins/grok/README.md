# sidekick + Grok

This integration gives [Grok](https://docs.x.ai/build) the Neovim hand-off
sidekick provides for Claude Code, Codex, opencode, pi, Crush, Amp, and
Antigravity:

- Before Grok runs its edit tools (`search_replace`, `write_file`), Grok
  invokes `sidekick hook`; if you have the target file open with unsaved
  changes in Neovim, the call is denied until you save.
- After each tool call, sidekick refreshes the buffer in every Neovim
  instance that has the file open.

Grok's docs describe its hooks as Claude-compatible, but the wire shape
actually diverges — camelCase envelope fields (`hookEventName`, `toolName`,
`toolInput`), snake_case event names (`pre_tool_use`), Grok-specific tool
names, and a top-level `{"decision":"deny"}` block response instead of
Claude's nested `permissionDecision`. sidekick handles all of it.

**No visual-selection injection.** Grok's `UserPromptSubmit` is a passive
event whose hook stdout is ignored, so there's no channel to add your Neovim
selection to a prompt. That gap is an upstream limitation, not sidekick
configuration.

## Install

The recommended path is:

```bash
sidekick init
```

or:

```bash
sidekick doctor --fix
```

Both commands write [`sidekick.json`](./sidekick.json) into your Grok hooks
directory after showing the diff:

- `~/.grok/hooks/sidekick.json` — applies globally
- `<project>/.grok/hooks/sidekick.json` — applies in one project

Grok merges every `*.json` file in those directories at startup, so sidekick
owns `sidekick.json` outright rather than editing a shared settings file —
nothing else of yours is touched. Project-level hooks require the
`/hooks-trust` permission the first time Grok sees them.

If `sidekick` is not on `PATH`, the hook fails non-blockingly and Grok
proceeds as if no hook had run. Install the binary first with
`cargo install sidekick`.

## How it works

Grok writes the hook payload to `sidekick hook`'s stdin and reads a JSON
decision back from stdout:

```jsonc
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "",
        "hooks": [{ "type": "command", "command": "sidekick hook" }]
      }
    ],
    "PostToolUse": [ /* same, refreshes the buffer after an edit lands */ ]
  }
}
```

The matcher is empty (match every tool) because Grok's tool names
(`search_replace`, `write_file`, `read_file`, ...) don't match Claude's
`Edit|Write|MultiEdit`; sidekick filters to file-edit tools server-side, so
`Bash`, reads, and MCP calls pass straight through. `PreToolUse` denies the
edit when the buffer is dirty (returning `{"decision":"deny","reason":...}`),
and `PostToolUse` triggers the buffer refresh.

## Verify

After installing, `sidekick doctor` shows a `Grok hook registered` row with
the config path it found.
