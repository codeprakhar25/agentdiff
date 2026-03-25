/**
 * agentdiff plugin for OpenCode
 *
 * Managed by `agentdiff init`.
 */
import type { Plugin } from "@opencode-ai/plugin"
import { dirname } from "path"

const CAPTURE_SCRIPT = "/home/prakh/.agentdiff/scripts/capture-opencode.py"
const FILE_EDIT_TOOLS = new Set(["edit", "write", "patch", "multiedit", "replace"])

type PendingEdit = {
  filePath: string
  repoDir: string
  sessionID: string
  tool: string
  args: Record<string, unknown>
}

export const AgentDiffPlugin: Plugin = async (ctx) => {
  const { $ } = ctx
  const pendingEdits = new Map<string, PendingEdit>()

  const findGitRepo = async (filePath: string): Promise<string | null> => {
    try {
      const dir = dirname(filePath)
      const result = await $`git -C ${dir} rev-parse --show-toplevel`.quiet()
      const repoRoot = result.stdout.toString().trim()
      return repoRoot || null
    } catch {
      return null
    }
  }

  return {
    "tool.execute.before": async (input, output) => {
      const tool = String(input.tool || "").toLowerCase()
      if (!FILE_EDIT_TOOLS.has(tool)) {
        return
      }

      const args = (output.args ?? {}) as Record<string, unknown>
      const filePath =
        (args.filePath as string | undefined) ??
        (args.file_path as string | undefined) ??
        (args.path as string | undefined)
      if (!filePath) {
        return
      }

      const repoDir = await findGitRepo(filePath)
      if (!repoDir) {
        return
      }

      pendingEdits.set(input.callID, {
        filePath,
        repoDir,
        sessionID: input.sessionID,
        tool,
        args,
      })
    },

    "tool.execute.after": async (input, _output) => {
      const editInfo = pendingEdits.get(input.callID)
      pendingEdits.delete(input.callID)
      if (!editInfo) {
        return
      }

      const { filePath, repoDir, sessionID, tool, args } = editInfo
      const payload = {
        hook_event_name: "PostToolUse",
        session_id: sessionID,
        model: String((input as any).modelID || "opencode"),
        cwd: repoDir,
        tool_name: tool,
        tool_input: {
          filePath,
          old_string: String(args.oldString ?? args.old_string ?? args.old ?? ""),
          new_string: String(args.newString ?? args.new_string ?? args.new ?? ""),
          content: String(args.content ?? ""),
        },
      }

      try {
        await $`echo ${JSON.stringify(payload)} | python3 ${CAPTURE_SCRIPT}`.quiet()
      } catch (error) {
        console.error("[agentdiff] OpenCode capture failed:", String(error))
      }
    },
  }
}

export default AgentDiffPlugin
