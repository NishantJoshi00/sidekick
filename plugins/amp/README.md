# sidekick + Amp

This plugin gives [Amp](https://ampcode.com) (Sourcegraph's coding agent) the
same Neovim integration sidekick provides for Claude Code, opencode, and pi:

- Before Amp runs `edit_file` or `create_file`, the plugin pipes a hook
  envelope to the `sidekick` binary; if you have the target file open with
  unsaved changes, the call is denied until you save.
- After each tool call, sidekick refreshes the buffer in every Neovim
  instance that has the file open.

Amp emits no `UserPromptSubmit`-equivalent event, so visual-selection
injection (which sidekick supports for Claude Code, opencode, and pi) is not
available here.

## Install

Requires the `sidekick` binary on `PATH` (see the top-level [README](../../README.md)).
If `sidekick` isn't found, the plugin is a no-op.

Drop [`sidekick.ts`](./sidekick.ts) into one of:

- `~/.config/amp/plugins/sidekick.ts` — applies globally
- `<project>/.amp/plugins/sidekick.ts` — applies in one project

```bash
mkdir -p ~/.config/amp/plugins
curl -sSL https://raw.githubusercontent.com/NishantJoshi00/sidekick/main/plugins/amp/sidekick.ts \
  -o ~/.config/amp/plugins/sidekick.ts
```

Amp discovers plugins from these directories at startup. After installing or
updating the file, run `plugins: reload` from Amp's command palette to pick
up the change.

## How it works

The plugin translates Amp's in-process plugin events into the `sidekick hook`
stdin/stdout protocol that already powers the Claude Code integration:

```
Amp tool.call    →  sidekick hook (PreToolUse)
Amp tool.result  →  sidekick hook (PostToolUse)
```

When sidekick replies with `permissionDecision: "deny"`, the `tool.call`
handler returns `{ action: "reject-and-continue", message }` to abort the
tool call without halting the thread. Any other response (including no
response) lets it proceed. Unlike opencode, Amp's `tool.result` event
carries the original tool arguments, so the buffer refresh needs no call-id
correlation. Every `sidekick hook` call has a hard 3-second timeout so a
stalled hook can never hang Amp.

The plugin maps Amp's lowercase tool names to the Claude-shape envelope
sidekick expects:

| Amp tool      | Sidekick tool |
| ------------- | ------------- |
| `edit_file`   | `Edit`        |
| `create_file` | `Write`       |

`Bash`, `Read`, and Amp's other built-in tools are pass-through — sidekick
only intercepts file-mutating tools.

## Verify

After installing, `sidekick doctor` shows an `Amp plugin installed` row with
the path it found.
