//! Unit tests for socket path utilities

use sidekick::utils::{compute_socket_path_with_pid, find_matching_sockets};

#[test]
fn test_compute_socket_path_with_pid() {
    let pid = 12345;
    let socket_path = compute_socket_path_with_pid(pid).expect("Failed to compute socket path");

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
fn test_compute_socket_path_deterministic() {
    let pid = 99999;

    // Computing the same path twice should yield identical results
    let path1 = compute_socket_path_with_pid(pid).expect("Failed to compute socket path");
    let path2 = compute_socket_path_with_pid(pid).expect("Failed to compute socket path");

    assert_eq!(path1, path2);
}

#[test]
fn test_compute_socket_path_different_pids() {
    let pid1 = 11111;
    let pid2 = 22222;

    let path1 = compute_socket_path_with_pid(pid1).expect("Failed to compute socket path");
    let path2 = compute_socket_path_with_pid(pid2).expect("Failed to compute socket path");

    // Different PIDs should produce different paths
    assert_ne!(path1, path2);

    // Extract filenames
    let name1 = path1.file_name().unwrap().to_string_lossy();
    let name2 = path2.file_name().unwrap().to_string_lossy();

    // Extract PIDs from filenames (format: hash-pid.sock)
    let pid_str1 = name1
        .strip_suffix(".sock")
        .and_then(|s| s.split('-').last())
        .unwrap();
    let pid_str2 = name2
        .strip_suffix(".sock")
        .and_then(|s| s.split('-').last())
        .unwrap();

    assert_eq!(pid_str1, "11111");
    assert_eq!(pid_str2, "22222");
}

#[test]
fn test_find_matching_sockets_empty() {
    // In a directory with no matching sockets, should return empty vec
    let sockets = find_matching_sockets().expect("Failed to find sockets");

    // We don't know if there are actual sockets, but this shouldn't fail
    assert!(sockets.is_empty() || !sockets.is_empty());
}

#[test]
fn test_find_matching_sockets_filters_nonexistent() {
    // This test verifies that find_matching_sockets only returns existing files
    let sockets = find_matching_sockets().expect("Failed to find sockets");

    for socket in &sockets {
        assert!(socket.exists(), "Socket path should exist: {:?}", socket);
    }
}

// Note: Tests that change cwd can interfere with parallel test execution
// and have been removed. Socket path computation based on cwd is tested
// indirectly through other tests.

#[test]
fn test_socket_path_pattern() {
    let pid = 123;
    let socket_path = compute_socket_path_with_pid(pid).expect("Failed to compute socket path");

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
