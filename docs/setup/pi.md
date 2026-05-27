# pi — manual setup

pi uses a TypeScript extension system. Like opencode, there is no config file to edit — drop the extension in, pi loads it at startup.

## Install

```bash
mkdir -p ~/.pi/agent/extensions
curl -sSL https://raw.githubusercontent.com/NishantJoshi00/sidekick/main/plugins/pi/sidekick.ts \
  -o ~/.pi/agent/extensions/sidekick.ts
```

For a per-project install, use `<project>/.pi/extensions/sidekick.ts`.

If the extension ever shows up disabled, enable it with `pi config`.

## How it works

The extension translates pi events into the `sidekick hook` stdin/stdout protocol:

```
pi tool_call    →  sidekick hook (PreToolUse)
pi tool_result  →  sidekick hook (PostToolUse)
pi input        →  sidekick hook (UserPromptSubmit)
```

When sidekick replies with `permissionDecision: "deny"`, the `tool_call` handler returns `{ block: true, reason }` to abort the tool call. Unlike opencode, pi's `tool_result` event carries the original tool arguments, so the buffer refresh needs no call-id correlation.

For `input`, sidekick returns the Neovim visual selection as `additionalContext`; pi has no separate context channel on a prompt, so the extension appends it to the submitted text. Inputs synthesised by other extensions are skipped. Every `sidekick hook` call has a hard timeout.

## Failure mode

If `sidekick` is not on `PATH`, the extension is a no-op rather than blocking pi. `sidekick doctor` is the way to confirm wiring.

## Verify

```bash
sidekick doctor
```

Look for the `pi extension installed` row with the path it found.
