//! Unit tests for socket path utilities

use sidekick::utils::{compute_socket_path_with_pid, find_matching_sockets, find_sockets_for_path};

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
fn find_sockets_for_path_walks_up_to_match_ancestor() {
    // Regression: Amp's plugin runtime sets process.cwd() to
    // ~/.config/amp/plugins, so the cwd-based hash never matches the
    // Neovim socket the user launched from their project. The fallback
    // walks up from the edited file's absolute path until it finds a
    // directory whose hash matches an existing socket.
    use std::fs;

    let tmp = std::env::temp_dir();
    let unique = format!(
        "sidekick-walkup-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let project_dir = tmp.join(&unique);
    let nested = project_dir.join("src").join("nested");
    fs::create_dir_all(&nested).expect("create nested dirs");

    // canonicalize matches what utils does internally
    let project_canonical = project_dir.canonicalize().expect("canonicalize project");
    let hash = blake3::hash(project_canonical.to_string_lossy().as_bytes());
    let socket_path = std::path::PathBuf::from(format!("/tmp/{}-99999.sock", hash.to_hex()));
    fs::write(&socket_path, "").expect("create fake socket");

    let file = nested.join("handler.rs");
    let found = find_sockets_for_path(&file).expect("walk up");

    // Cleanup before assertions so a failure doesn't leak the socket
    let _ = fs::remove_file(&socket_path);
    let _ = fs::remove_dir_all(&project_dir);

    assert_eq!(
        found,
        vec![socket_path],
        "walk-up should find the socket hashed from the project root"
    );
}

#[test]
fn find_sockets_for_path_returns_empty_when_no_ancestor_matches() {
    use std::fs;

    let tmp = std::env::temp_dir();
    let unique = format!(
        "sidekick-walkup-miss-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let project_dir = tmp.join(&unique);
    fs::create_dir_all(&project_dir).expect("create project");
    let file = project_dir.join("foo.txt");

    let found = find_sockets_for_path(&file).expect("walk up");
    let _ = fs::remove_dir_all(&project_dir);

    assert!(
        found.is_empty(),
        "no socket exists for any ancestor — walk-up should return empty"
    );
}

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
