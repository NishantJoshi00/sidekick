# sidekick + Antigravity

This integration gives Google's [Antigravity CLI](https://antigravity.google/)
(`agy`) the same Neovim hand-off sidekick provides for Claude Code, Codex,
opencode, pi, Crush, and Amp:

- Before Antigravity runs `write_to_file`, `replace_file_content`, or
  `multi_replace_file_content`, the host invokes `sidekick hook`; if you have
  the target file open with unsaved changes in Neovim, the call is denied
  until you save.

Antigravity emits `PostToolUse` after each edit but the payload omits the
tool name and arguments, so sidekick can't refresh a single known file. It
broadcasts `:checktime` to every connected Neovim instead: clean buffers
reload from disk, dirty buffers stay protected by Vim's W12 prompt. The
result is the same end-to-end experience as Claude Code's per-file refresh,
without needing the host to tell us which file changed.

One piece of the sidekick contract is still intentionally missing:

- **No visual-selection injection on prompt submit.** Antigravity has no
  user-prompt-submit event. `PreInvocation` fires before every model call,
  so injecting on it would repeat the selection on every agentic-loop turn.

That gap is an upstream limitation, not sidekick configuration.

## Install

The recommended path is:

```bash
sidekick init
```

or:

```bash
sidekick doctor --fix
```

Both commands merge the snippet from [`hooks.json`](./hooks.json) into your
existing `~/.gemini/config/hooks.json` (preserving the rest of your config)
after showing the diff.

If `sidekick` is not on `PATH`, the hook fails non-blockingly and Antigravity
proceeds as if no hook had run. Install the binary first with
`cargo install sidekick`.

## How it works

Antigravity loads hooks from `hooks.json`. The merged config registers
`sidekick hook` against both `PreToolUse` (the deny gate) and `PostToolUse`
(the `:checktime` broadcast), with a regex matcher scoped to file-edit
tools so neither event fires for `run_command` or read tools:

```jsonc
{
  "sidekick": {
    "PreToolUse": [
      {
        "matcher": "^(write_to_file|replace_file_content|multi_replace_file_content)$",
        "hooks": [
          {
            "type": "command",
            "command": "sidekick hook",
            "timeout": 10
          }
        ]
      }
    ],
    "PostToolUse": [
      {
        "matcher": "^(write_to_file|replace_file_content|multi_replace_file_content)$",
        "hooks": [
          {
            "type": "command",
            "command": "sidekick hook",
            "timeout": 10
          }
        ]
      }
    ]
  }
}
```

Antigravity writes the hook payload to `sidekick hook`'s stdin and reads a
JSON decision back from stdout. Antigravity's wire shape differs from Claude
Code's and Crush's: there is no top-level event-name tag, fields are
`camelCase` (`conversationId`, `workspacePaths`, `toolCall`), the tool call
is nested (`toolCall.name` + `toolCall.args`), and tool-argument keys are
`PascalCase` (`TargetFile`). Sidekick accepts all three shapes.

On `PreToolUse`, sidekick replies with a top-level `decision` (Antigravity's
wire convention, distinct from Claude's nested `hookSpecificOutput`).
On `PostToolUse`, sidekick replies with `{}` and runs `:checktime` against
every Neovim instance reachable from the conversation's workspace.

## Verify

After installing, `sidekick doctor` shows an `antigravity hook registered`
row with the config path it found.
