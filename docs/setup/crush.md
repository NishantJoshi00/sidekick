# Crush — manual setup

Crush registers hooks directly in its own JSON config — no plugin file to bundle.

## Edit `~/.config/crush/crush.json`

Merge this into your existing config, preserving every other key you have set:

```jsonc
{
  "$schema": "https://charm.land/crush.json",
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "^(edit|write|multiedit)$",
        "command": "sidekick hook",
        "timeout": 10
      }
    ]
  }
}
```

The matcher is scoped to file-edit tools so sidekick is not invoked for every `bash` or MCP call — a small latency win.

## What's missing on Crush

Crush only emits `PreToolUse`, so two pieces of the sidekick contract are absent here:

- **No post-edit buffer refresh.** Crush has no `PostToolUse` event. After a Crush edit lands on disk, your Neovim buffer will not auto-reload. Use `:checktime` or set `autoread` if you want it picked up automatically.
- **No visual-selection injection on prompt submit.** Crush has no `UserPromptSubmit` equivalent.

Both are upstream limitations, not sidekick configuration.

## Failure mode

If `sidekick` is not on `PATH`, the hook fails non-blockingly and Crush proceeds as if no hook had run. You will see no errors and no protection.

## Verify

```bash
sidekick doctor
```

Look for the `crush hook registered` row with the config path it found.
