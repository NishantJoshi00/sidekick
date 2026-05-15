# sidekick + opencode

This plugin lets [opencode](https://opencode.ai) respect the same Neovim
integration that protects Claude Code users:

- Before opencode runs `edit` or `write`, the plugin pipes a hook envelope to
  the `sidekick` binary; if you have the target file open with unsaved
  changes, the call is blocked until you save.
- After each tool call, sidekick refreshes the buffer in every Neovim
  instance that has the file open.
- When you submit a prompt, your current Neovim visual selection is appended
  to the message as context.

## Install

Requires the `sidekick` binary on `PATH` (see the top-level [README](../../README.md)).
If `sidekick` isn't found, the plugin is a no-op.

Drop [`sidekick.ts`](./sidekick.ts) into one of:

- `~/.config/opencode/plugin/sidekick.ts` — applies globally
- `<project>/.opencode/plugin/sidekick.ts` — applies in one project

```bash
mkdir -p ~/.config/opencode/plugin
curl -sSL https://raw.githubusercontent.com/NishantJoshi00/sidekick/main/plugins/opencode/sidekick.ts \
  -o ~/.config/opencode/plugin/sidekick.ts
```

opencode loads it at startup — no config file, no registration step.

## How it works

The plugin translates opencode's plugin events into the `sidekick hook`
stdin/stdout protocol that already powers the Claude Code integration:

```
opencode tool.execute.before  →  sidekick hook (PreToolUse)
opencode tool.execute.after   →  sidekick hook (PostToolUse)
opencode chat.message         →  sidekick hook (UserPromptSubmit)
```

When sidekick replies with `permissionDecision: "deny"`, the plugin throws to
abort the tool call. Any other response (including no response) lets it
proceed. Because `tool.execute.after` doesn't carry tool arguments, the plugin
correlates the two events by opencode's `callID`.

For `chat.message`, sidekick returns the Neovim visual selection as
`additionalContext`; the plugin appends it to the user's existing text part
(opencode validates message parts against a strict schema, so a brand-new
part can't be synthesised). Every `sidekick hook` call has a hard timeout so
a stalled hook can never hang opencode.

## Verify

After installing, `sidekick doctor` shows an `opencode plugin installed` row
with the path it found.
