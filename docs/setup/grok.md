# Grok — manual setup

xAI's [Grok CLI](https://docs.x.ai/build) (`grok`) merges every `*.json` file
in its hooks directory at startup, so sidekick ships its own
`sidekick.json` rather than editing a shared settings file.

## Drop in `~/.grok/hooks/sidekick.json`

The hook is also read from `.grok/hooks/sidekick.json` per project, which
takes precedence over the global file. Project-level hooks require the
`/hooks-trust` permission the first time Grok sees them.

```json
{
  "hooks": {
    "PreToolUse": [
      { "matcher": "", "hooks": [{ "type": "command", "command": "sidekick hook" }] }
    ],
    "PostToolUse": [
      { "matcher": "", "hooks": [{ "type": "command", "command": "sidekick hook" }] }
    ]
  }
}
```

Grok's docs describe its hooks as Claude-compatible, but the wire shape
actually diverges: the envelope uses camelCase fields (`hookEventName`,
`toolName`, `toolInput`), the event names are snake_case (`pre_tool_use`),
the edit tool is `search_replace`, and a blocking hook returns a top-level
`{"decision":"deny"}` rather than Claude's nested `permissionDecision`.
sidekick handles all of this. The matcher is left empty (match every tool)
because Grok's tool names don't match `Edit|Write|MultiEdit`; sidekick
filters to file-edit tools itself, so non-edit tool calls pass straight
through.

## What's missing on Grok

Grok's `UserPromptSubmit` is a passive event — its hook stdout is ignored,
so there's no channel to inject your Neovim visual selection into the
prompt. That one piece of the sidekick contract is an upstream limitation,
not configuration. Edit denial and post-edit buffer refresh both work.

## Failure mode

If `sidekick` is not on `PATH`, the hook fails non-blockingly and Grok
proceeds as if no hook had run. You will see no errors and no protection.

## Verify

```bash
sidekick doctor
```

Look for the `Grok hook registered` row with the config path it found.
