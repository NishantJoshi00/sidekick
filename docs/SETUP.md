# Manual setup

Most people should just run:

```bash
cargo install sidekick
sidekick init
```

`init` walks every AI harness it finds, shows you the diff for each change, and writes only what you accept. `sidekick doctor --fix` is the same set of writes in repair-mode, one prompt per row.

This document is for the people who want to do it themselves — to audit the writes, to script the rollout, or to debug a row that `doctor` keeps flagging red.

## The two things every setup needs

1. **The binary on `PATH`.** Every plugin and hook calls `sidekick` by name. If it's not found, the integration either no-ops (opencode, pi) or fails noisily (Claude Code, Codex, Crush).

   ```bash
   cargo install sidekick
   which sidekick
   ```

2. **The `nvim` alias.** Sidekick needs to know which Neovim instance you opened so its hook can find the socket. The alias routes `nvim` through `sidekick neovim`, which launches `nvim --listen /tmp/<blake3(cwd)>-<pid>.sock`.

   ```bash
   # ~/.zshrc or ~/.bashrc
   alias nvim='sidekick neovim'
   ```

   If you do not want an alias, call `sidekick neovim <file>` directly. Plain `nvim` will not be discoverable by the hook.

## Per-harness wiring

Each harness has its own setup page. They cover what file gets written where, what the contents are, and how to verify with `sidekick doctor`.

- [Claude Code](setup/claude-code.md)
- [Codex](setup/codex.md)
- [opencode](setup/opencode.md)
- [pi](setup/pi.md)
- [Crush](setup/crush.md)
- [Amp](setup/amp.md)

## Verifying

`sidekick doctor` is the source of truth. After every manual step:

```bash
sidekick doctor
```

A row with `✓` is wired. A row with `✗` is what to fix next.
