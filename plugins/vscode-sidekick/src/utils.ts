import * as fs from 'fs';
import * as blake3 from 'blake3';

/**
 * Compute the socket path based on the current working directory.
 * Uses blake3 hash of the canonicalized path, same as the Rust implementation.
 *
 * Pattern: /tmp/<blake3(cwd)>-vscode-<pid>.sock
 */
export function computeSocketPath(cwd: string): string {
    const hash = computeCwdHash(cwd);
    const pid = process.pid;
    return `/tmp/${hash}-vscode-${pid}.sock`;
}

/**
 * Compute blake3 hash of the canonicalized working directory.
 * This matches the Rust implementation exactly:
 *   blake3::hash(cwd_absolute.to_string_lossy().as_bytes()).to_hex()
 */
export function computeCwdHash(cwd: string): string {
    // Resolve to absolute path (similar to canonicalize in Rust)
    const absolutePath = fs.realpathSync(cwd);

    // Hash using blake3, same as Rust side
    const hash = blake3.hash(absolutePath);

    // Convert to hex string (64 characters)
    return hash.toString('hex');
}
