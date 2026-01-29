/**
 * Mock implementation of the vscode module for testing.
 * This provides the minimum interface needed to test our handlers.
 */

export interface TextDocument {
    uri: { fsPath: string };
    isDirty: boolean;
    getText(range?: Range): string;
}

export interface Range {
    start: Position;
    end: Position;
}

export interface Position {
    line: number;
    character: number;
}

export interface Selection extends Range {
    isEmpty: boolean;
}

export interface TextEditor {
    document: TextDocument;
    selection: Selection;
}

export interface WorkspaceFolder {
    uri: { fsPath: string };
}

// Mock state that can be manipulated in tests
export const mockState = {
    textDocuments: [] as TextDocument[],
    activeTextEditor: undefined as TextEditor | undefined,
    workspaceFolders: undefined as WorkspaceFolder[] | undefined,
    lastWarningMessage: undefined as string | undefined,
    lastExecutedCommand: undefined as { command: string; args: unknown[] } | undefined
};

// Reset mock state between tests
export function resetMockState(): void {
    mockState.textDocuments = [];
    mockState.activeTextEditor = undefined;
    mockState.workspaceFolders = undefined;
    mockState.lastWarningMessage = undefined;
    mockState.lastExecutedCommand = undefined;
}

export const workspace = {
    get textDocuments(): TextDocument[] {
        return mockState.textDocuments;
    },
    get workspaceFolders(): WorkspaceFolder[] | undefined {
        return mockState.workspaceFolders;
    }
};

export const window = {
    get activeTextEditor(): TextEditor | undefined {
        return mockState.activeTextEditor;
    },
    get visibleTextEditors(): TextEditor[] {
        return mockState.activeTextEditor ? [mockState.activeTextEditor] : [];
    },
    showWarningMessage(message: string): Thenable<string | undefined> {
        mockState.lastWarningMessage = message;
        return Promise.resolve(undefined);
    }
};

export const commands = {
    executeCommand(command: string, ...args: unknown[]): Thenable<unknown> {
        mockState.lastExecutedCommand = { command, args };
        return Promise.resolve(undefined);
    }
};

export const Uri = {
    file(path: string): { fsPath: string } {
        return { fsPath: path };
    }
};
