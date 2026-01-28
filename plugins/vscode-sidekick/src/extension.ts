import * as vscode from 'vscode';
import { IPCServer } from './server';
import { computeSocketPath } from './utils';

let server: IPCServer | undefined;

export function activate(context: vscode.ExtensionContext) {
    console.log('Sidekick extension activating...');

    // Start the IPC server
    startServer(context);

    // Register commands
    context.subscriptions.push(
        vscode.commands.registerCommand('sidekick.showSocketPath', () => {
            if (server) {
                vscode.window.showInformationMessage(`Sidekick socket: ${server.getSocketPath()}`);
            } else {
                vscode.window.showWarningMessage('Sidekick server is not running');
            }
        })
    );

    context.subscriptions.push(
        vscode.commands.registerCommand('sidekick.restart', () => {
            stopServer();
            startServer(context);
            vscode.window.showInformationMessage('Sidekick server restarted');
        })
    );

    // Restart server when workspace folders change
    context.subscriptions.push(
        vscode.workspace.onDidChangeWorkspaceFolders(() => {
            stopServer();
            startServer(context);
        })
    );
}

function startServer(context: vscode.ExtensionContext) {
    const workspaceFolder = vscode.workspace.workspaceFolders?.[0];
    if (!workspaceFolder) {
        console.log('No workspace folder found, server not started');
        return;
    }

    const cwd = workspaceFolder.uri.fsPath;
    const socketPath = computeSocketPath(cwd);

    server = new IPCServer(socketPath);
    server.start();

    console.log(`Sidekick server started at ${socketPath}`);

    // Clean up socket on deactivation
    context.subscriptions.push({
        dispose: () => stopServer()
    });
}

function stopServer() {
    if (server) {
        server.stop();
        server = undefined;
    }
}

export function deactivate() {
    stopServer();
}
