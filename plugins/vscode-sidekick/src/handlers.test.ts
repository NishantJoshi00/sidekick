import { handleRequest, _testing, RPCRequest, RPCResponse } from './handlers';
import { mockState, resetMockState, TextDocument, TextEditor } from './__mocks__/vscode';

const { isBufferStatusParams, isRefreshBufferParams, isSendMessageParams } = _testing;

describe('handlers', () => {
    beforeEach(() => {
        resetMockState();
    });

    describe('type guards', () => {
        describe('isBufferStatusParams', () => {
            it('should return true for valid params', () => {
                expect(isBufferStatusParams({ file_path: '/path/to/file' })).toBe(true);
            });

            it('should return false for null', () => {
                expect(isBufferStatusParams(null)).toBe(false);
            });

            it('should return false for undefined', () => {
                expect(isBufferStatusParams(undefined)).toBe(false);
            });

            it('should return false for missing file_path', () => {
                expect(isBufferStatusParams({})).toBe(false);
            });

            it('should return false for non-string file_path', () => {
                expect(isBufferStatusParams({ file_path: 123 })).toBe(false);
            });
        });

        describe('isRefreshBufferParams', () => {
            it('should return true for valid params', () => {
                expect(isRefreshBufferParams({ file_path: '/path/to/file' })).toBe(true);
            });

            it('should return false for invalid params', () => {
                expect(isRefreshBufferParams(null)).toBe(false);
                expect(isRefreshBufferParams({})).toBe(false);
                expect(isRefreshBufferParams({ file_path: 123 })).toBe(false);
            });
        });

        describe('isSendMessageParams', () => {
            it('should return true for valid params', () => {
                expect(isSendMessageParams({ message: 'Hello' })).toBe(true);
            });

            it('should return false for invalid params', () => {
                expect(isSendMessageParams(null)).toBe(false);
                expect(isSendMessageParams({})).toBe(false);
                expect(isSendMessageParams({ message: 123 })).toBe(false);
            });
        });
    });

    describe('handleRequest', () => {
        describe('unknown method', () => {
            it('should return error for unknown method', () => {
                const request: RPCRequest = {
                    id: 1,
                    method: 'unknown_method'
                };

                const response = handleRequest(request);

                expect(response.id).toBe(1);
                expect(response.error).toBeDefined();
                expect(response.error?.code).toBe(-32601);
                expect(response.error?.message).toContain('Method not found');
            });
        });

        describe('buffer_status', () => {
            it('should return error for missing file_path', () => {
                const request: RPCRequest = {
                    id: 1,
                    method: 'buffer_status',
                    params: {}
                };

                const response = handleRequest(request);

                expect(response.error).toBeDefined();
                expect(response.error?.code).toBe(-32602);
            });

            it('should return is_current=false, has_unsaved_changes=false for unopened file', () => {
                const request: RPCRequest = {
                    id: 1,
                    method: 'buffer_status',
                    params: { file_path: '/path/to/unopened/file.txt' }
                };

                const response = handleRequest(request);

                expect(response.result).toEqual({
                    is_current: false,
                    has_unsaved_changes: false
                });
            });

            it('should return correct status for open document', () => {
                // Setup mock document
                const mockDoc: TextDocument = {
                    uri: { fsPath: '/path/to/file.txt' },
                    isDirty: true,
                    getText: () => 'content'
                };
                mockState.textDocuments = [mockDoc];
                mockState.activeTextEditor = {
                    document: mockDoc,
                    selection: { start: { line: 0, character: 0 }, end: { line: 0, character: 0 }, isEmpty: true }
                };

                const request: RPCRequest = {
                    id: 1,
                    method: 'buffer_status',
                    params: { file_path: '/path/to/file.txt' }
                };

                const response = handleRequest(request);

                expect(response.result).toEqual({
                    is_current: true,
                    has_unsaved_changes: true
                });
            });

            it('should return is_current=false when file is open but not active', () => {
                const mockDoc: TextDocument = {
                    uri: { fsPath: '/path/to/file.txt' },
                    isDirty: false,
                    getText: () => 'content'
                };
                const otherDoc: TextDocument = {
                    uri: { fsPath: '/path/to/other.txt' },
                    isDirty: false,
                    getText: () => 'other content'
                };
                mockState.textDocuments = [mockDoc, otherDoc];
                mockState.activeTextEditor = {
                    document: otherDoc,
                    selection: { start: { line: 0, character: 0 }, end: { line: 0, character: 0 }, isEmpty: true }
                };

                const request: RPCRequest = {
                    id: 1,
                    method: 'buffer_status',
                    params: { file_path: '/path/to/file.txt' }
                };

                const response = handleRequest(request);

                expect(response.result).toEqual({
                    is_current: false,
                    has_unsaved_changes: false
                });
            });
        });

        describe('refresh_buffer', () => {
            it('should return error for missing file_path', () => {
                const request: RPCRequest = {
                    id: 1,
                    method: 'refresh_buffer',
                    params: {}
                };

                const response = handleRequest(request);

                expect(response.error).toBeDefined();
                expect(response.error?.code).toBe(-32602);
            });

            it('should return success for unopened file', () => {
                const request: RPCRequest = {
                    id: 1,
                    method: 'refresh_buffer',
                    params: { file_path: '/path/to/unopened/file.txt' }
                };

                const response = handleRequest(request);

                expect(response.result).toEqual({ success: true });
            });

            it('should execute revert command for open document', () => {
                const mockDoc: TextDocument = {
                    uri: { fsPath: '/path/to/file.txt' },
                    isDirty: true,
                    getText: () => 'content'
                };
                mockState.textDocuments = [mockDoc];
                mockState.activeTextEditor = {
                    document: mockDoc,
                    selection: { start: { line: 0, character: 0 }, end: { line: 0, character: 0 }, isEmpty: true }
                };

                const request: RPCRequest = {
                    id: 1,
                    method: 'refresh_buffer',
                    params: { file_path: '/path/to/file.txt' }
                };

                const response = handleRequest(request);

                expect(response.result).toEqual({ success: true });
                expect(mockState.lastExecutedCommand?.command).toBe('workbench.action.files.revert');
            });
        });

        describe('send_message', () => {
            it('should return error for missing message', () => {
                const request: RPCRequest = {
                    id: 1,
                    method: 'send_message',
                    params: {}
                };

                const response = handleRequest(request);

                expect(response.error).toBeDefined();
                expect(response.error?.code).toBe(-32602);
            });

            it('should show warning message', () => {
                const request: RPCRequest = {
                    id: 1,
                    method: 'send_message',
                    params: { message: 'Test message' }
                };

                const response = handleRequest(request);

                expect(response.result).toEqual({ success: true });
                expect(mockState.lastWarningMessage).toBe('Sidekick: Test message');
            });
        });

        describe('get_visual_selection', () => {
            it('should return null when no active editor', () => {
                mockState.activeTextEditor = undefined;

                const request: RPCRequest = {
                    id: 1,
                    method: 'get_visual_selection'
                };

                const response = handleRequest(request);

                expect(response.result).toBeNull();
            });

            it('should return null when selection is empty', () => {
                const mockDoc: TextDocument = {
                    uri: { fsPath: '/path/to/file.txt' },
                    isDirty: false,
                    getText: () => ''
                };
                mockState.activeTextEditor = {
                    document: mockDoc,
                    selection: {
                        start: { line: 0, character: 0 },
                        end: { line: 0, character: 0 },
                        isEmpty: true
                    }
                };

                const request: RPCRequest = {
                    id: 1,
                    method: 'get_visual_selection'
                };

                const response = handleRequest(request);

                expect(response.result).toBeNull();
            });

            it('should return selection context when text is selected', () => {
                const mockDoc: TextDocument = {
                    uri: { fsPath: '/path/to/file.txt' },
                    isDirty: false,
                    getText: () => 'selected text'
                };
                mockState.activeTextEditor = {
                    document: mockDoc,
                    selection: {
                        start: { line: 5, character: 0 },
                        end: { line: 10, character: 15 },
                        isEmpty: false
                    }
                };

                const request: RPCRequest = {
                    id: 1,
                    method: 'get_visual_selection'
                };

                const response = handleRequest(request);

                expect(response.result).toEqual({
                    file_path: '/path/to/file.txt',
                    start_line: 6, // 1-indexed
                    end_line: 11,  // 1-indexed
                    content: 'selected text'
                });
            });
        });
    });

    describe('request ID handling', () => {
        it('should preserve numeric request ID', () => {
            const request: RPCRequest = {
                id: 42,
                method: 'buffer_status',
                params: { file_path: '/test.txt' }
            };

            const response = handleRequest(request);
            expect(response.id).toBe(42);
        });

        it('should preserve string request ID', () => {
            const request: RPCRequest = {
                id: 'my-request-id',
                method: 'buffer_status',
                params: { file_path: '/test.txt' }
            };

            const response = handleRequest(request);
            expect(response.id).toBe('my-request-id');
        });

        it('should preserve null request ID', () => {
            const request: RPCRequest = {
                id: null,
                method: 'buffer_status',
                params: { file_path: '/test.txt' }
            };

            const response = handleRequest(request);
            expect(response.id).toBeNull();
        });
    });
});
