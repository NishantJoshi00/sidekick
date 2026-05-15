# sidekick + pi

This extension lets the [pi coding agent](https://github.com/badlogic/pi-mono)
respect the same Neovim integration that protects Claude Code and opencode
users:

- Before pi runs `edit` or `write`, the extension pipes a hook envelope to the
  `sidekick` binary; if you have the target file open with unsaved changes,
  the call is blocked until you save.
- After each tool call, sidekick refreshes the buffer in every Neovim
  instance that has the file open.
- When you submit a prompt, your current Neovim visual selection is appended
  to the message as context.

## Install

Requires the `sidekick` binary on `PATH` (see the top-level [README](../../README.md)).
If `sidekick` isn't found, the extension is a no-op.

Drop [`sidekick.ts`](./sidekick.ts) into one of:

- `~/.pi/agent/extensions/sidekick.ts` — applies globally
- `<project>/.pi/extensions/sidekick.ts` — applies in one project

```bash
mkdir -p ~/.pi/agent/extensions
curl -sSL https://raw.githubusercontent.com/NishantJoshi00/sidekick/main/plugins/pi/sidekick.ts \
  -o ~/.pi/agent/extensions/sidekick.ts
```

pi loads extensions from these directories at startup — no registration step.
If it ever shows up disabled, enable it with `pi config`.

## How it works

The extension translates pi's extension events into the `sidekick hook`
stdin/stdout protocol that already powers the Claude Code integration:

```
pi tool_call    →  sidekick hook (PreToolUse)
pi tool_result  →  sidekick hook (PostToolUse)
pi input        →  sidekick hook (UserPromptSubmit)
```

When sidekick replies with `permissionDecision: "deny"`, the `tool_call`
handler returns `{ block: true, reason }` to abort the tool call. Any other
response (including no response) lets it proceed. Unlike opencode, pi's
`tool_result` event carries the original tool arguments, so the buffer
refresh needs no call-id correlation.

For `input`, sidekick returns the Neovim visual selection as
`additionalContext`; pi has no separate context channel on a prompt, so the
extension appends it to the submitted text (the same approach the opencode
bridge takes). Inputs synthesised by other extensions are skipped. Every
`sidekick hook` call has a hard timeout so a stalled hook can never hang pi.

## Verify

After installing, `sidekick doctor` shows a `pi extension installed` row with
the path it found.
