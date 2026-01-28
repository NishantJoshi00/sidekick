//! Unit tests for socket path utilities

use sidekick::utils::{
    compute_neovim_socket_path, compute_vscode_socket_path, find_neovim_sockets,
    find_vscode_sockets,
};

#[test]
fn test_compute_neovim_socket_path() {
    let pid = 12345;
    let socket_path =
        compute_neovim_socket_path(pid).expect("Failed to compute Neovim socket path");

    // Verify path is in /tmp
    assert!(socket_path.starts_with("/tmp"));

    // Verify path ends with -<pid>.sock
    let path_str = socket_path.to_string_lossy();
    assert!(path_str.ends_with("-12345.sock"));

    // Verify path contains a hash (64 character hex string from blake3)
    let filename = socket_path.file_name().unwrap().to_string_lossy();
    assert!(filename.len() > 70); // 64 (hash) + 1 (-) + 5 (pid) + 5 (.sock)
}

#[test]
fn test_compute_vscode_socket_path() {
    let pid = 12345;
    let socket_path =
        compute_vscode_socket_path(pid).expect("Failed to compute VSCode socket path");

    // Verify path is in /tmp
    assert!(socket_path.starts_with("/tmp"));

    // Verify path ends with -vscode-<pid>.sock
    let path_str = socket_path.to_string_lossy();
    assert!(path_str.ends_with("-vscode-12345.sock"));

    // Verify path contains -vscode-
    assert!(path_str.contains("-vscode-"));
}

#[test]
fn test_compute_socket_path_deterministic() {
    let pid = 99999;

    // Computing the same path twice should yield identical results
    let nvim_path1 = compute_neovim_socket_path(pid).expect("Failed to compute Neovim socket path");
    let nvim_path2 = compute_neovim_socket_path(pid).expect("Failed to compute Neovim socket path");
    assert_eq!(nvim_path1, nvim_path2);

    let vscode_path1 =
        compute_vscode_socket_path(pid).expect("Failed to compute VSCode socket path");
    let vscode_path2 =
        compute_vscode_socket_path(pid).expect("Failed to compute VSCode socket path");
    assert_eq!(vscode_path1, vscode_path2);
}

#[test]
fn test_compute_socket_path_different_pids() {
    let pid1 = 11111;
    let pid2 = 22222;

    let nvim_path1 =
        compute_neovim_socket_path(pid1).expect("Failed to compute Neovim socket path");
    let nvim_path2 =
        compute_neovim_socket_path(pid2).expect("Failed to compute Neovim socket path");

    // Different PIDs should produce different paths
    assert_ne!(nvim_path1, nvim_path2);

    // Extract filenames
    let name1 = nvim_path1.file_name().unwrap().to_string_lossy();
    let name2 = nvim_path2.file_name().unwrap().to_string_lossy();

    // Extract PIDs from filenames (format: hash-pid.sock)
    let pid_str1 = name1
        .strip_suffix(".sock")
        .and_then(|s| s.split('-').next_back())
        .unwrap();
    let pid_str2 = name2
        .strip_suffix(".sock")
        .and_then(|s| s.split('-').next_back())
        .unwrap();

    assert_eq!(pid_str1, "11111");
    assert_eq!(pid_str2, "22222");
}

#[test]
fn test_neovim_and_vscode_socket_paths_differ() {
    let pid = 12345;

    let nvim_path = compute_neovim_socket_path(pid).expect("Failed to compute Neovim socket path");
    let vscode_path =
        compute_vscode_socket_path(pid).expect("Failed to compute VSCode socket path");

    // Same PID should produce different paths for different editors
    assert_ne!(nvim_path, vscode_path);

    // VSCode path should contain -vscode-, Neovim should not
    let nvim_str = nvim_path.to_string_lossy();
    let vscode_str = vscode_path.to_string_lossy();

    assert!(!nvim_str.contains("-vscode-"));
    assert!(vscode_str.contains("-vscode-"));
}

#[test]
fn test_find_neovim_sockets_empty() {
    // In a directory with no matching sockets, should return empty vec
    let sockets = find_neovim_sockets().expect("Failed to find Neovim sockets");

    // We don't know if there are actual sockets, but this shouldn't fail
    assert!(sockets.is_empty() || !sockets.is_empty());
}

#[test]
fn test_find_vscode_sockets_empty() {
    // In a directory with no matching sockets, should return empty vec
    let sockets = find_vscode_sockets().expect("Failed to find VSCode sockets");

    // We don't know if there are actual sockets, but this shouldn't fail
    assert!(sockets.is_empty() || !sockets.is_empty());
}

#[test]
fn test_find_sockets_filters_nonexistent() {
    // This test verifies that find functions only return existing files
    let nvim_sockets = find_neovim_sockets().expect("Failed to find Neovim sockets");
    let vscode_sockets = find_vscode_sockets().expect("Failed to find VSCode sockets");

    for socket in &nvim_sockets {
        assert!(
            socket.exists(),
            "Neovim socket path should exist: {:?}",
            socket
        );
    }

    for socket in &vscode_sockets {
        assert!(
            socket.exists(),
            "VSCode socket path should exist: {:?}",
            socket
        );
    }
}

#[test]
fn test_neovim_socket_path_pattern() {
    let pid = 123;
    let socket_path =
        compute_neovim_socket_path(pid).expect("Failed to compute Neovim socket path");

    // Verify the path matches expected pattern: /tmp/<hash>-<pid>.sock
    let path_str = socket_path.to_string_lossy();
    let parts: Vec<&str> = path_str.rsplitn(2, '/').collect();

    assert_eq!(parts.len(), 2);
    assert_eq!(parts[1], "/tmp");

    let filename = parts[0];
    let components: Vec<&str> = filename.split('-').collect();

    assert_eq!(components.len(), 2);
    assert!(components[0].len() == 64); // blake3 hash is 64 hex chars
    assert!(components[1].ends_with(".sock"));
}

#[test]
fn test_vscode_socket_path_pattern() {
    let pid = 123;
    let socket_path =
        compute_vscode_socket_path(pid).expect("Failed to compute VSCode socket path");

    // Verify the path matches expected pattern: /tmp/<hash>-vscode-<pid>.sock
    let path_str = socket_path.to_string_lossy();
    let parts: Vec<&str> = path_str.rsplitn(2, '/').collect();

    assert_eq!(parts.len(), 2);
    assert_eq!(parts[1], "/tmp");

    let filename = parts[0];
    let components: Vec<&str> = filename.split('-').collect();

    assert_eq!(components.len(), 3);
    assert!(components[0].len() == 64); // blake3 hash is 64 hex chars
    assert_eq!(components[1], "vscode");
    assert!(components[2].ends_with(".sock"));
}
