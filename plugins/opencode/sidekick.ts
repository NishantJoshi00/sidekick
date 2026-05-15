// sidekick opencode plugin
//
// Bridges opencode's plugin events to the `sidekick hook` binary so opencode
// respects the same Neovim integration that protects Claude Code users:
//   - tool.execute.before  → block edits to dirty buffers
//   - tool.execute.after   → refresh buffers after an edit lands
//   - chat.message         → inject the current Neovim visual selection
//
// Install: drop this file into ~/.config/opencode/plugin/sidekick.ts
//          (per-project: <project>/.opencode/plugin/sidekick.ts)
//
// Requires `sidekick` on PATH. If it isn't, the plugin silently no-ops
// rather than blocking opencode.

import type { Plugin } from "@opencode-ai/plugin"
import { spawn } from "node:child_process"
import { isAbsolute, join } from "node:path"

type ToolName = "Edit" | "Write"

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

// A hook must never hang opencode. If `sidekick hook` doesn't answer within
// this window (e.g. its RPC to Neovim stalls), we kill it and move on.
const HOOK_TIMEOUT_MS = 3000

function pickFilePath(args: unknown): string | null {
  if (!args || typeof args !== "object") return null
  const candidate = (args as Record<string, unknown>).filePath
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

    // Hard ceiling: if sidekick stalls, kill it so opencode never blocks.
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

export const SidekickPlugin: Plugin = async ({ directory }) => {
  const cwd = directory ?? process.cwd()

  // opencode's tool.execute.after doesn't carry tool args, so we stash the
  // resolved file path under the call id when the call is allowed through,
  // then look it up afterward to drive the buffer refresh.
  const pendingByCallID = new Map<string, { tool: ToolName; filePath: string }>()

  return {
    "tool.execute.before": async (input, output) => {
      const toolName = TOOL_MAP[input.tool]
      if (!toolName) return

      const raw = pickFilePath(output.args)
      if (!raw) return
      const filePath = isAbsolute(raw) ? raw : join(cwd, raw)

      const response = await callSidekick(
        {
          session_id: input.sessionID,
          transcript_path: "",
          cwd,
          hook_event_name: "PreToolUse",
          tool_name: toolName,
          tool_input: { file_path: filePath },
        },
        cwd,
      )

      if (response?.hookSpecificOutput?.permissionDecision === "deny") {
        throw new Error(
          response.hookSpecificOutput.permissionDecisionReason ??
            "sidekick: file has unsaved changes in Neovim",
        )
      }

      pendingByCallID.set(input.callID, { tool: toolName, filePath })
    },

    "tool.execute.after": async (input) => {
      const pending = pendingByCallID.get(input.callID)
      if (!pending) return
      pendingByCallID.delete(input.callID)

      await callSidekick(
        {
          session_id: input.sessionID,
          transcript_path: "",
          cwd,
          hook_event_name: "PostToolUse",
          tool_name: pending.tool,
          tool_input: { file_path: pending.filePath },
        },
        cwd,
      )
    },

    "chat.message": async (input, output) => {
      const response = await callSidekick(
        {
          session_id: input.sessionID,
          transcript_path: "",
          cwd,
          hook_event_name: "UserPromptSubmit",
          prompt: "",
        },
        cwd,
      )

      const context = response?.hookSpecificOutput?.additionalContext
      if (!context) return

      // opencode validates message parts against a strict schema (id,
      // sessionID and messageID are all required). Rather than synthesise a
      // new part, append the selection to the user's existing text part.
      const textPart = output.parts.find((p: any) => p.type === "text") as any
      if (textPart && typeof textPart.text === "string") {
        textPart.text = `${textPart.text}\n\n${context}`
      }
    },
  }
}

export default SidekickPlugin
