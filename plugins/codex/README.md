# sidekick + Codex

This plugin gives Codex the same Neovim integration sidekick provides for
Claude Code, opencode, and pi:

- Before Codex runs a file edit or `apply_patch`, the plugin runs
  `sidekick hook` so dirty current Neovim buffers can block the edit.
- After an edit lands, the same hook refreshes open Neovim buffers.
- On prompt submission, a live or recent Neovim visual selection can be added
  as prompt context.

## Install

The recommended path is:

```bash
sidekick init
```

or:

```bash
sidekick doctor --fix
```

Both commands install the Codex plugin into your personal Codex marketplace,
enable `plugin_hooks`, and enable `sidekick@personal` in `~/.codex/config.toml`
after showing the diff.

If `sidekick` is not on `PATH`, the hook command fails and Codex reports the
hook failure. Install the binary first with `cargo install sidekick`.

## How it works

Codex loads `hooks.json` from this plugin. The hook config invokes the existing
`sidekick hook` stdin/stdout protocol for `PreToolUse`, `PostToolUse`, and
`UserPromptSubmit`.

Codex may ask you to trust new or changed hooks before they run. Review the
hook in Codex's hooks UI and trust it once.
