// sidekick pi extension
//
// Bridges the pi coding agent's extension events to the `sidekick hook`
// binary so pi respects the same Neovim integration that protects Claude
// Code and opencode users:
//   - tool_call    → block edits to files with unsaved Neovim changes
//   - tool_result  → refresh buffers after an edit lands
//   - input        → inject the current Neovim visual selection
//
// Install: drop this file into ~/.pi/agent/extensions/sidekick.ts
//          (per-project: <project>/.pi/extensions/sidekick.ts)
//
// Requires `sidekick` on PATH. If it isn't, the extension silently no-ops
// rather than blocking pi.

import type { ExtensionAPI } from "@mariozechner/pi-coding-agent"
import { spawn } from "node:child_process"
import { isAbsolute, join } from "node:path"

type ToolName = "Edit" | "Write"

// pi's built-in tools are lowercase; only the two that mutate a file matter.
// pi's `edit` takes a batch of edits, but the hook handler treats Edit and
// MultiEdit identically, so mapping it to "Edit" is enough.
const TOOL_MAP: Record<string, ToolName> = {
  edit: "Edit",
  write: "Write",
}

type ToolEnvelope = {
  session_id: string
  transcript_path: string
  cwd: string
  hook_event_name: "PreToolUse" | "PostToolUse"
  tool_name: ToolName
  tool_input: { file_path: string }
}

type PromptEnvelope = {
  session_id: string
  transcript_path: string
  cwd: string
  hook_event_name: "UserPromptSubmit"
  prompt: string
}

type HookEnvelope = ToolEnvelope | PromptEnvelope

type HookResponse = {
  hookSpecificOutput?: {
    permissionDecision?: "allow" | "deny" | "ask"
    permissionDecisionReason?: string
    additionalContext?: string
  }
}

// A hook must never hang pi. If `sidekick hook` doesn't answer within this
// window (e.g. its RPC to Neovim stalls), we kill it and move on.
const HOOK_TIMEOUT_MS = 3000

// pi's edit/write tools name the target `path`; it may be relative to cwd.
function pickFilePath(input: unknown): string | null {
  if (!input || typeof input !== "object") return null
  const candidate = (input as Record<string, unknown>).path
  return typeof candidate === "string" && candidate.length > 0 ? candidate : null
}

function callSidekick(envelope: HookEnvelope, cwd: string): Promise<HookResponse | null> {
  return new Promise((resolve) => {
    let proc
    try {
      proc = spawn("sidekick", ["hook"], { stdio: ["pipe", "pipe", "ignore"], cwd })
    } catch {
      resolve(null)
      return
    }

    let settled = false
    const finish = (value: HookResponse | null) => {
      if (settled) return
      settled = true
      clearTimeout(timer)
      resolve(value)
    }

    // Hard ceiling: if sidekick stalls, kill it so pi never blocks.
    const timer = setTimeout(() => {
      proc.kill("SIGKILL")
      finish(null)
    }, HOOK_TIMEOUT_MS)

    let stdout = ""
    proc.stdout.on("data", (chunk: Buffer) => {
      stdout += chunk.toString()
    })
    proc.on("error", () => finish(null))
    proc.on("close", () => {
      const body = stdout.trim()
      if (!body) {
        finish({})
        return
      }
      try {
        finish(JSON.parse(body) as HookResponse)
      } catch {
        finish(null)
      }
    })

    // A killed child can EPIPE on stdin; swallow it rather than crash.
    proc.stdin.on("error", () => {})
    proc.stdin.write(JSON.stringify(envelope))
    proc.stdin.end()
  })
}

export default function (pi: ExtensionAPI) {
  // Before pi runs `edit`/`write`, ask sidekick whether the file is safe to
  // touch. A `deny` becomes a `{ block }` return, which aborts the tool call.
  pi.on("tool_call", async (event, ctx) => {
    const toolName = TOOL_MAP[event.toolName]
    if (!toolName) return

    const raw = pickFilePath(event.input)
    if (!raw) return
    const filePath = isAbsolute(raw) ? raw : join(ctx.cwd, raw)

    const response = await callSidekick(
      {
        session_id: "",
        transcript_path: "",
        cwd: ctx.cwd,
        hook_event_name: "PreToolUse",
        tool_name: toolName,
        tool_input: { file_path: filePath },
      },
      ctx.cwd,
    )

    if (response?.hookSpecificOutput?.permissionDecision === "deny") {
      return {
        block: true,
        reason:
          response.hookSpecificOutput.permissionDecisionReason ??
          "sidekick: file has unsaved changes in Neovim",
      }
    }
  })

  // After the edit lands, refresh the buffer in every Neovim instance that
  // has the file open. Unlike opencode, pi's tool_result carries the original
  // tool arguments, so no call-id correlation is needed.
  pi.on("tool_result", async (event, ctx) => {
    const toolName = TOOL_MAP[event.toolName]
    if (!toolName) return

    const raw = pickFilePath(event.input)
    if (!raw) return
    const filePath = isAbsolute(raw) ? raw : join(ctx.cwd, raw)

    await callSidekick(
      {
        session_id: "",
        transcript_path: "",
        cwd: ctx.cwd,
        hook_event_name: "PostToolUse",
        tool_name: toolName,
        tool_input: { file_path: filePath },
      },
      ctx.cwd,
    )
  })

  // On prompt submission, append the current Neovim visual selection.
  pi.on("input", async (event, ctx) => {
    // Only real submissions carry a selection intent; skip inputs synthesised
    // by other extensions to avoid feedback loops.
    if (event.source === "extension") return

    const response = await callSidekick(
      {
        session_id: "",
        transcript_path: "",
        cwd: ctx.cwd,
        hook_event_name: "UserPromptSubmit",
        prompt: "",
      },
      ctx.cwd,
    )

    const context = response?.hookSpecificOutput?.additionalContext
    if (!context) return

    // pi has no separate "additional context" channel on a prompt, so the
    // selection is appended to the submitted text — the same approach the
    // opencode bridge takes. Appended after a blank line, it can't be
    // mistaken for a leading skill/template command.
    return { action: "transform", text: `${event.text}\n\n${context}` }
  })
}
