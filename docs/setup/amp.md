# Amp — manual setup

[Amp](https://ampcode.com) (Sourcegraph's coding agent) uses a TypeScript plugin system. Like opencode and pi, there is no config file to edit — drop the plugin in, Amp loads it at startup.

## Install

```bash
mkdir -p ~/.config/amp/plugins
curl -sSL https://raw.githubusercontent.com/NishantJoshi00/sidekick/main/plugins/amp/sidekick.ts \
  -o ~/.config/amp/plugins/sidekick.ts
```

For a per-project install, use `<project>/.amp/plugins/sidekick.ts`.

After installing or updating the file, run `plugins: reload` from Amp's command palette so the new code is picked up — Amp does not auto-reload plugin files.

## How it works

The plugin translates Amp events into the `sidekick hook` stdin/stdout protocol:

```
Amp tool.call    →  sidekick hook (PreToolUse)
Amp tool.result  →  sidekick hook (PostToolUse)
```

When sidekick replies with `permissionDecision: "deny"`, the `tool.call` handler returns `{ action: "reject-and-continue", message }` to abort the tool call without halting the thread. Unlike opencode, Amp's `tool.result` event carries the original tool arguments, so the buffer refresh needs no call-id correlation.

Amp emits no `UserPromptSubmit`-equivalent event, so visual-selection injection (which sidekick supports for Claude Code, Codex, opencode, and pi) is not available here. Every `sidekick hook` call has a hard 3-second timeout.

The plugin maps Amp's lowercase tool names to the Claude-shape envelope sidekick expects:

| Amp tool      | Sidekick tool |
| ------------- | ------------- |
| `edit_file`   | `Edit`        |
| `create_file` | `Write`       |

`Bash`, `Read`, and Amp's other built-in tools are pass-through.

## Failure mode

If `sidekick` is not on `PATH`, the plugin is a no-op rather than blocking Amp. `sidekick doctor` is the way to confirm wiring.

## Verify

```bash
sidekick doctor
```

Look for the `Amp plugin installed` row with the path it found.
