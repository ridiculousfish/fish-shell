use std::env;
use std::os::unix::fs::symlink;

fn main() {
    let manifest_relative = |path| {
        let dir = env::var("CARGO_MANIFEST_DIR").unwrap();
        format!("{}/{}", dir, path)
    };

    let fish_src_dir = env::var("FISH_SRC_DIR").unwrap_or(manifest_relative("../src/"));
    let fish_build_dir = env::var("FISH_BUILD_DIR").unwrap_or(manifest_relative("../build/"));

    let source_files = vec!["src/fishffi.rs", "src/topic_monitor.rs"];
    cxx_build::bridges(source_files)
        .compiler("/usr/bin/clang++")
        .flag_if_supported("-std=c++11")
        .include(fish_build_dir)
        .include(fish_src_dir)
        .compile("fish-rust-cxx");

    // cxx won't actually put its headers anywhere reasonable. Symlink them to where we're asked.
    if let Ok(headers_out) = env::var("FISH_HEADER_OUTDIR") {
        let _ = std::fs::create_dir(&headers_out);
        let mut headers = env::var("OUT_DIR").unwrap();
        headers.push_str("/cxxbridge/include");
        let _ = symlink(headers, &format!("{}/{}", headers_out, "include"));
    }
}
