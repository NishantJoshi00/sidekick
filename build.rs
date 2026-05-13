//! Build-time fetch of the bundled asciinema cast for `sidekick demo`.
//!
//! The cast is hosted on asciinema (the URL below). We download it once at
//! build time, write it to `OUT_DIR/demo.cast`, and `demo.rs` embeds it via
//! `include_bytes!`. This keeps the repo free of binary assets while still
//! shipping an offline-playable demo.
//!
//! To update the demo: re-record, upload to asciinema, paste the new
//! `.cast` URL below, and cut a release.
//!
//! Override at build time with `SIDEKICK_DEMO_CAST_URL` if you need to
//! test against a different cast without editing this file.

use std::env;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

const DEFAULT_DEMO_CAST_URL: &str = "https://asciinema.org/a/746395.cast";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=SIDEKICK_DEMO_CAST_URL");

    let url =
        env::var("SIDEKICK_DEMO_CAST_URL").unwrap_or_else(|_| DEFAULT_DEMO_CAST_URL.to_string());

    let out_dir = env::var("OUT_DIR").expect("OUT_DIR not set by cargo");
    let dest = Path::new(&out_dir).join("demo.cast");

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(15))
        .build();

    let response = agent
        .get(&url)
        .call()
        .unwrap_or_else(|err| panic!("couldn't download demo from {url}: {err}"));

    let mut body = Vec::new();
    response
        .into_reader()
        .take(10 * 1024 * 1024) // 10 MiB hard cap, casts are typically <100 KiB
        .read_to_end(&mut body)
        .expect("couldn't read demo response");

    if body.is_empty() {
        panic!("demo at {url} was empty");
    }

    fs::write(&dest, &body).expect("couldn't write demo to OUT_DIR");
}
