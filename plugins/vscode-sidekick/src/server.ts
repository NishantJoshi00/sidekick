import * as net from 'net';
import * as fs from 'fs';
import { handleRequest, RPCRequest, RPCResponse } from './handlers';

/**
 * IPC Server that listens on a Unix socket for JSON-RPC requests from the Sidekick CLI.
 */
export class IPCServer {
    private server: net.Server | null = null;
    private socketPath: string;

    constructor(socketPath: string) {
        this.socketPath = socketPath;
    }

    getSocketPath(): string {
        return this.socketPath;
    }

    start(): void {
        // Clean up any existing socket file
        this.cleanupSocket();

        this.server = net.createServer((socket) => {
            this.handleConnection(socket);
        });

        this.server.on('error', (err) => {
            console.error('Sidekick IPC server error:', err);
            if ((err as NodeJS.ErrnoException).code === 'EADDRINUSE') {
                // Socket file exists, try to clean up and restart
                this.cleanupSocket();
                setTimeout(() => this.start(), 100);
            }
        });

        this.server.listen(this.socketPath, () => {
            console.log(`Sidekick IPC server listening on ${this.socketPath}`);
        });
    }

    stop(): void {
        if (this.server) {
            this.server.close();
            this.server = null;
        }
        this.cleanupSocket();
    }

    private cleanupSocket(): void {
        try {
            if (fs.existsSync(this.socketPath)) {
                fs.unlinkSync(this.socketPath);
            }
        } catch (err) {
            console.error('Failed to cleanup socket:', err);
        }
    }

    private handleConnection(socket: net.Socket): void {
        let buffer = '';

        socket.on('data', (data) => {
            buffer += data.toString();

            // Try to parse complete JSON messages
            // Messages are newline-delimited
            const lines = buffer.split('\n');
            buffer = lines.pop() || ''; // Keep incomplete line in buffer

            for (const line of lines) {
                if (line.trim()) {
                    this.handleMessage(line, socket);
                }
            }
        });

        socket.on('error', (err) => {
            console.error('Sidekick socket error:', err);
        });

        socket.on('close', () => {
            // Connection closed
        });
    }

    private handleMessage(message: string, socket: net.Socket): void {
        try {
            const request: RPCRequest = JSON.parse(message);
            const response = handleRequest(request);

            // Send response as newline-delimited JSON
            socket.write(JSON.stringify(response) + '\n');
        } catch (err) {
            console.error('Failed to handle message:', err);
            const errorResponse: RPCResponse = {
                id: null,
                error: {
                    code: -32700,
                    message: 'Parse error'
                }
            };
            socket.write(JSON.stringify(errorResponse) + '\n');
        }
    }
}
