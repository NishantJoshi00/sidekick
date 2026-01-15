# Philosophy

**Your AI Assistant Should Be a Sidekick, Not the Pilot.**

## The Core Principle

When you have unsaved changes in a file, you are actively writing code. That file is yours. The AI should wait.

This isn't about distrust of AI tools. It's about respecting the human in the loop. AI-assisted coding works best when the human remains the author — making decisions, shaping the code, staying engaged. The moment AI can silently overwrite your active work, you become a passive observer of your own codebase.

Sidekick enforces a simple boundary: **if you're working on it, AI waits.**

## Why This Matters

AI coding tools are powerful. They can refactor, generate, and modify code faster than any human. But speed without coordination creates chaos.

The best collaborations have clear boundaries:
- You're editing `main.rs` with unsaved changes? That's your space.
- You saved and moved on? AI can proceed.
- You're actively typing? AI defers.

This isn't a limitation — it's how productive human-AI collaboration should work.

## Roadmap

Sidekick currently supports Neovim + Claude Code. The vision is broader.

### Phase 1: More AI Tools

Extend protection to other in-terminal AI coding assistants:
- Aider
- Continue
- Goose
- Other CLI-based AI tools with hook/plugin systems

The pattern is the same: intercept file modification requests, check editor state, allow or block.

### Phase 2: More Editors

Extend protection to other editors with RPC or plugin capabilities:
- Helix
- Emacs
- VS Code (via Neovim plugin or native extension)
- Zed

Each editor needs:
1. A way to launch with a known socket/IPC path
2. A way to query buffer state (current file, modified status)
3. A way to refresh buffers after external changes

### Phase 3: Universal Protocol

Long-term: a standardized protocol for AI tools to query editor state before modifying files. Instead of each tool implementing its own hooks, editors could expose a simple API:

```
GET /buffer/status?file=/path/to/file
→ { "open": true, "modified": true, "active": true }
```

AI tools check before writing. Editors respond with state. No overwrites, no lost work.

## Contributing

Want to help extend Sidekick? The architecture is designed for this:

- **New AI tools**: Implement the hook handler for your tool's extension system
- **New editors**: Implement the `Action` trait for your editor's RPC/IPC protocol
- **Protocol design**: Help design the universal query protocol

See [CONTRIBUTING.md](CONTRIBUTING.md) or open an issue to discuss.
