//! This app's `zeren` is the Zeren app's command line (crates/cli), as
//! production's `zeren` is (apps/zeren/tests/cli.rs runs the same checks
//! against that binary): every subcommand's help as the library renders it,
//! the version, and the engine's subcommands handed to the engine helper
//! beside the binary with the same arguments, exit status passed through.

use std::path::{Path, PathBuf};
use std::process::Command;

/// A scratch folder of this test's own.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zgx-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The binary as `zeren` in `dir` (a hard link: it runs from there, so the
/// helper beside it is the one it finds, and clap names it `zeren`).
fn zeren_in(dir: &Path) -> PathBuf {
    let link = dir.join("zeren");
    std::fs::hard_link(env!("CARGO_BIN_EXE_zeren-zgx"), &link)
        .or_else(|_| std::fs::copy(env!("CARGO_BIN_EXE_zeren-zgx"), &link).map(|_| ()))
        .unwrap();
    link
}

#[test]
fn every_subcommand_s_help_is_the_shared_command_line_s() {
    let dir = scratch("help");
    let zeren = zeren_in(&dir);
    let version = env!("ZEREN_APP_VERSION");
    for path in zeren_cli::subcommand_paths(version) {
        let out = Command::new(&zeren)
            .args(&path)
            .arg("--help")
            .env("ZEREN_DATA_DIR", &dir)
            .output()
            .unwrap();
        assert!(out.status.success(), "{path:?}: {out:?}");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim_end(),
            zeren_cli::help_for(version, &path).trim_end(),
            "zeren {} --help",
            path.join(" ")
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn version_is_zeren_and_this_build_s() {
    let out = Command::new(env!("CARGO_BIN_EXE_zeren-zgx")).arg("--version").output().unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        format!("zeren {}", env!("ZEREN_APP_VERSION"))
    );
}

/// `zeren plugin list` runs in the helper: the same arguments, and its exit
/// status is the command's (exec: the helper is this process).
#[cfg(unix)]
#[test]
fn the_engine_s_subcommands_run_in_the_helper_beside_the_app() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = scratch("helper");
    let zeren = zeren_in(&dir);
    let heard = dir.join("heard");
    let helper = dir.join("Zeren Engine");
    std::fs::write(&helper, format!("#!/bin/sh\necho \"$@\" > '{}'\nexit 7\n", heard.display())).unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let status = Command::new(&zeren).args(["plugin", "list"]).env("ZEREN_DATA_DIR", &dir).status().unwrap();
    assert_eq!(status.code(), Some(7));
    assert_eq!(std::fs::read_to_string(&heard).unwrap().trim(), "plugin list");
    // Without a helper, it says so (and opens no window).
    std::fs::remove_file(&helper).unwrap();
    let out = Command::new(&zeren).arg("status").env("ZEREN_DATA_DIR", &dir).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("couldn't start the engine"), "{out:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
