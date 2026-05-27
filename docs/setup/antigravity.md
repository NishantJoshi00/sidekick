# Antigravity — manual setup

Google's [Antigravity CLI](https://antigravity.google/) (`agy`) registers hooks directly in its own JSON config — no plugin file to bundle.

## Edit `~/.gemini/config/hooks.json`

Merge this into your existing config, preserving every other top-level hook entry you have set. The hook is also accepted at `.agents/hooks.json` per project, which takes precedence over the global file.

```jsonc
{
  "sidekick": {
    "PreToolUse": [
      {
        "matcher": "^(write_to_file|replace_file_content|multi_replace_file_content)$",
        "hooks": [
          { "type": "command", "command": "sidekick hook", "timeout": 10 }
        ]
      }
    ],
    "PostToolUse": [
      {
        "matcher": "^(write_to_file|replace_file_content|multi_replace_file_content)$",
        "hooks": [
          { "type": "command", "command": "sidekick hook", "timeout": 10 }
        ]
      }
    ]
  }
}
```

The matcher is scoped to file-edit tools so sidekick is not invoked for every `run_command` or read — a small latency win.

## What's missing on Antigravity

Antigravity has no user-prompt-submit event, so one piece of the sidekick contract is absent here:

- **No visual-selection injection on prompt submit.** `PreInvocation` fires before every model call, not just on user submission, so injecting on it would repeat the selection on every agentic-loop turn.

That gap is an upstream limitation, not sidekick configuration. Post-edit buffer refresh works via `:checktime` — Antigravity's `PostToolUse` payload omits the tool name and args, so sidekick can't target a single file, but the broadcast checktime reloads any clean buffer whose disk content changed and leaves dirty buffers protected by Vim's W12 prompt.

## Failure mode

If `sidekick` is not on `PATH`, the hook fails non-blockingly and Antigravity proceeds as if no hook had run. You will see no errors and no protection.

## Verify

```bash
sidekick doctor
```

Look for the `antigravity hook registered` row with the config path it found.
