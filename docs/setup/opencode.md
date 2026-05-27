# opencode — manual setup

opencode uses a TypeScript plugin system. There is no config file to edit — drop the file in, opencode loads it at startup.

## Install

```bash
mkdir -p ~/.config/opencode/plugin
curl -sSL https://raw.githubusercontent.com/NishantJoshi00/sidekick/main/plugins/opencode/sidekick.ts \
  -o ~/.config/opencode/plugin/sidekick.ts
```

For a per-project install instead of global, drop it at `<project>/.opencode/plugin/sidekick.ts`.

## How it works

The plugin translates opencode events into the `sidekick hook` stdin/stdout protocol:

```
opencode tool.execute.before  →  sidekick hook (PreToolUse)
opencode tool.execute.after   →  sidekick hook (PostToolUse)
opencode chat.message         →  sidekick hook (UserPromptSubmit)
```

When sidekick replies with `permissionDecision: "deny"`, the plugin throws to abort the tool call. Any other response (including no response) lets it proceed. The plugin correlates before/after events by opencode's `callID`.

`apply_patch` carries a multi-file patch rather than a `filePath`, so the plugin parses the patch's `*** Add/Update/Delete File:` and `*** Move to:` markers and checks (and refreshes) every file the patch touches.

For `chat.message`, sidekick returns the Neovim visual selection as `additionalContext` and the plugin appends it to the user's existing text part. Every `sidekick hook` call has a hard timeout, so a stalled hook can never hang opencode.

## Failure mode

If `sidekick` is not on `PATH`, the plugin is a no-op rather than blocking opencode. You will see no errors and no protection — `sidekick doctor` is the way to confirm wiring.

## Verify

```bash
sidekick doctor
```

Look for the `opencode plugin installed` row with the path it found.
