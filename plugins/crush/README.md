# sidekick + Crush

This integration gives [Crush](https://charm.land/) the same Neovim hand-off
sidekick provides for Claude Code, Codex, opencode, and pi:

- Before Crush runs `edit`, `write`, or `multiedit`, Crush invokes
  `sidekick hook`; if you have the target file open with unsaved changes in
  Neovim, the call is denied until you save.

Crush currently emits only `PreToolUse` hooks, so two pieces of the sidekick
contract are intentionally missing here:

- **No post-edit buffer refresh.** Crush has no `PostToolUse` event. After a
  Crush edit lands on disk, your Neovim buffer will not auto-reload — use
  `:checktime` or set `autoread` if you want it picked up automatically.
- **No visual-selection injection on prompt submit.** Crush has no
  `UserPromptSubmit` equivalent.

Both gaps are upstream limitations, not sidekick configuration.

## Install

The recommended path is:

```bash
sidekick init
```

or:

```bash
sidekick doctor --fix
```

Both commands merge the snippet from [`crush.json`](./crush.json) into your
existing `~/.config/crush/crush.json` (preserving the rest of your config)
after showing the diff.

If `sidekick` is not on `PATH`, the hook fails non-blockingly and Crush
proceeds as if no hook had run. Install the binary first with
`cargo install sidekick`.

## How it works

Crush loads hooks from `crush.json`. The merged config registers `sidekick hook`
against Crush's `PreToolUse` event with a regex matcher scoped to file-edit
tools:

```jsonc
{
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

Crush writes the hook payload to `sidekick hook`'s stdin and reads a JSON
decision back from stdout. Crush's payload shape differs from Claude Code's
on two points: the event-name key is `event` instead of `hook_event_name`,
and `transcript_path` is omitted entirely. Sidekick accepts both shapes.

The matcher is scoped to file-edit tools so sidekick is not invoked for every
`bash` or MCP tool call — a small win for latency.

## Verify

After installing, `sidekick doctor` shows a `crush hook registered` row with
the config path it found.
