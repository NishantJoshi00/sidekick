import * as vscode from 'vscode';
import * as path from 'path';
import * as fs from 'fs';

/**
 * JSON-RPC request structure
 */
export interface RPCRequest {
    id: number | string | null;
    method: string;
    params?: Record<string, unknown>;
}

/**
 * JSON-RPC response structure
 */
export interface RPCResponse {
    id: number | string | null;
    result?: unknown;
    error?: {
        code: number;
        message: string;
    };
}

/**
 * Buffer status request params
 */
interface BufferStatusParams {
    file_path: string;
}

/**
 * Refresh buffer request params
 */
interface RefreshBufferParams {
    file_path: string;
}

/**
 * Send message request params
 */
interface SendMessageParams {
    message: string;
}

/**
 * Buffer status response
 */
interface BufferStatusResult {
    is_current: boolean;
    has_unsaved_changes: boolean;
}

/**
 * Visual selection context
 */
interface EditorContext {
    file_path: string;
    start_line: number;
    end_line: number;
    content: string;
}

/**
 * Type guard to check if params match BufferStatusParams
 */
function isBufferStatusParams(params: unknown): params is BufferStatusParams {
    return (
        typeof params === 'object' &&
        params !== null &&
        'file_path' in params &&
        typeof (params as BufferStatusParams).file_path === 'string'
    );
}

/**
 * Type guard to check if params match RefreshBufferParams
 */
function isRefreshBufferParams(params: unknown): params is RefreshBufferParams {
    return (
        typeof params === 'object' &&
        params !== null &&
        'file_path' in params &&
        typeof (params as RefreshBufferParams).file_path === 'string'
    );
}

/**
 * Type guard to check if params match SendMessageParams
 */
function isSendMessageParams(params: unknown): params is SendMessageParams {
    return (
        typeof params === 'object' &&
        params !== null &&
        'message' in params &&
        typeof (params as SendMessageParams).message === 'string'
    );
}

/**
 * Handle an incoming RPC request and return a response
 */
export function handleRequest(request: RPCRequest): RPCResponse {
    try {
        switch (request.method) {
            case 'buffer_status':
                return handleBufferStatus(request);
            case 'refresh_buffer':
                return handleRefreshBuffer(request);
            case 'send_message':
                return handleSendMessage(request);
            case 'get_visual_selection':
                return handleGetVisualSelection(request);
            default:
                return {
                    id: request.id,
                    error: {
                        code: -32601,
                        message: `Method not found: ${request.method}`
                    }
                };
        }
    } catch (err) {
        return {
            id: request.id,
            error: {
                code: -32603,
                message: `Internal error: ${err}`
            }
        };
    }
}

/**
 * Check if a file has unsaved changes and is the current buffer
 */
function handleBufferStatus(request: RPCRequest): RPCResponse {
    if (!isBufferStatusParams(request.params)) {
        return {
            id: request.id,
            error: {
                code: -32602,
                message: 'Missing or invalid file_path parameter'
            }
        };
    }

    const filePath = request.params.file_path;

    // Resolve to absolute path
    const absolutePath = resolveFilePath(filePath);

    // Find the document in open text documents
    const document = vscode.workspace.textDocuments.find(doc => {
        try {
            const docPath = fs.realpathSync(doc.uri.fsPath);
            return docPath === absolutePath;
        } catch {
            return doc.uri.fsPath === absolutePath;
        }
    });

    if (!document) {
        // File is not open in any editor
        const result: BufferStatusResult = {
            is_current: false,
            has_unsaved_changes: false
        };
        return {
            id: request.id,
            result
        };
    }

    // Check if this document is in the active editor
    const activeEditor = vscode.window.activeTextEditor;
    const isCurrent = activeEditor?.document === document;

    const result: BufferStatusResult = {
        is_current: isCurrent ?? false,
        has_unsaved_changes: document.isDirty
    };
    return {
        id: request.id,
        result
    };
}

/**
 * Refresh a buffer by reloading it from disk
 */
function handleRefreshBuffer(request: RPCRequest): RPCResponse {
    if (!isRefreshBufferParams(request.params)) {
        return {
            id: request.id,
            error: {
                code: -32602,
                message: 'Missing or invalid file_path parameter'
            }
        };
    }

    const filePath = request.params.file_path;
    const absolutePath = resolveFilePath(filePath);

    // Find the document
    const document = vscode.workspace.textDocuments.find(doc => {
        try {
            const docPath = fs.realpathSync(doc.uri.fsPath);
            return docPath === absolutePath;
        } catch {
            return doc.uri.fsPath === absolutePath;
        }
    });

    if (!document) {
        // File is not open, nothing to refresh
        return {
            id: request.id,
            result: { success: true }
        };
    }

    // Find all editors showing this document and refresh
    const editors = vscode.window.visibleTextEditors.filter(e => e.document === document);

    if (editors.length > 0) {
        // Execute revert command for the document
        // This reloads the file from disk
        vscode.commands.executeCommand('workbench.action.files.revert', document.uri)
            .then(() => {
                console.log(`Refreshed buffer: ${filePath}`);
            }, (err: unknown) => {
                console.error(`Failed to refresh buffer: ${err}`);
            });
    }

    return {
        id: request.id,
        result: { success: true }
    };
}

/**
 * Send a notification message to the user
 */
function handleSendMessage(request: RPCRequest): RPCResponse {
    if (!isSendMessageParams(request.params)) {
        return {
            id: request.id,
            error: {
                code: -32602,
                message: 'Missing or invalid message parameter'
            }
        };
    }

    const message = request.params.message;

    // Show warning message (similar to Neovim's vim.notify with WARN level)
    vscode.window.showWarningMessage(`Sidekick: ${message}`);

    return {
        id: request.id,
        result: { success: true }
    };
}

/**
 * Get the current visual selection from the active editor
 */
function handleGetVisualSelection(request: RPCRequest): RPCResponse {
    const activeEditor = vscode.window.activeTextEditor;

    if (!activeEditor) {
        return {
            id: request.id,
            result: null
        };
    }

    const selection = activeEditor.selection;

    // Check if there's an actual selection (not just cursor position)
    if (selection.isEmpty) {
        return {
            id: request.id,
            result: null
        };
    }

    const document = activeEditor.document;
    const selectedText = document.getText(selection);

    // Get absolute file path
    let filePath: string;
    try {
        filePath = fs.realpathSync(document.uri.fsPath);
    } catch {
        filePath = document.uri.fsPath;
    }

    const context: EditorContext = {
        file_path: filePath,
        start_line: selection.start.line + 1, // Convert to 1-indexed
        end_line: selection.end.line + 1,
        content: selectedText
    };

    return {
        id: request.id,
        result: context
    };
}

/**
 * Resolve a file path to an absolute path
 */
function resolveFilePath(filePath: string): string {
    // If already absolute, canonicalize it
    if (path.isAbsolute(filePath)) {
        try {
            return fs.realpathSync(filePath);
        } catch {
            return filePath;
        }
    }

    // Relative path - resolve against workspace folder
    const workspaceFolders = vscode.workspace.workspaceFolders;
    const workspaceFolder = workspaceFolders?.[0];
    if (workspaceFolder) {
        const absolutePath = path.join(workspaceFolder.uri.fsPath, filePath);
        try {
            return fs.realpathSync(absolutePath);
        } catch {
            return absolutePath;
        }
    }

    return filePath;
}

// Export for testing
export const _testing = {
    isBufferStatusParams,
    isRefreshBufferParams,
    isSendMessageParams,
    resolveFilePath
};
