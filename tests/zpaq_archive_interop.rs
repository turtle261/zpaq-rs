#![cfg(unix)]

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use zpaq_rs::{zpaq_add, zpaq_command, zpaq_list};

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn run_ok<I, S>(program: &Path, args: I)
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new(program)
        .args(args)
        .output()
        .expect("run command");
    assert!(
        output.status.success(),
        "command failed: status={:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn ensure_zpaq_cli(root: &Path) -> PathBuf {
    let zpaq_dir = root.join("zpaq");
    let output = Command::new("make")
        .current_dir(&zpaq_dir)
        .arg("zpaq")
        .output()
        .expect("run make zpaq");
    assert!(
        output.status.success(),
        "make zpaq failed: status={:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let bin = zpaq_dir.join("zpaq");
    assert!(bin.exists(), "expected zpaq binary at {bin:?}");
    bin
}

fn find_file_named(root: &Path, name: &str) -> Option<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = fs::read_dir(&dir).ok()?;
        for entry in entries {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .file_name()
                .and_then(|s| s.to_str())
                .map(|s| s == name)
                .unwrap_or(false)
            {
                return Some(path);
            }
        }
    }
    None
}

#[test]
fn archive_add_append_list_extract_interop_matches_cli() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let zpaq_bin = ensure_zpaq_cli(&root);

    let temp = unique_temp_dir("zpaq-rs-interop");
    let src_dir = temp.join("dataset");
    fs::create_dir_all(&src_dir).expect("create dataset dir");

    let alpha = src_dir.join("alpha.txt");
    let beta = src_dir.join("beta.txt");
    fs::write(&alpha, b"alpha alpha alpha\n").expect("write alpha");
    fs::write(&beta, b"beta beta beta\n").expect("write beta");

    let archive_cli = temp.join("cli.zpaq");
    let archive_rust = temp.join("rust.zpaq");

    let archive_cli_s = archive_cli.to_string_lossy().to_string();
    let archive_rust_s = archive_rust.to_string_lossy().to_string();
    let src_dir_s = src_dir.to_string_lossy().to_string();

    run_ok(
        &zpaq_bin,
        [
            OsStr::new("add"),
            OsStr::new(&archive_cli_s),
            OsStr::new(&src_dir_s),
            OsStr::new("-method"),
            OsStr::new("3"),
            OsStr::new("-threads"),
            OsStr::new("2"),
        ],
    );

    zpaq_add(&archive_rust_s, &[&src_dir_s], "3", 2).expect("rust add");

    let size_cli_first = fs::metadata(&archive_cli).expect("stat cli archive").len();
    let size_rust_first = fs::metadata(&archive_rust)
        .expect("stat rust archive")
        .len();
    assert_eq!(
        size_cli_first, size_rust_first,
        "first add archive size differs"
    );

    let gamma = src_dir.join("gamma.txt");
    fs::write(&gamma, b"gamma append payload\n").expect("write gamma");

    run_ok(
        &zpaq_bin,
        [
            OsStr::new("add"),
            OsStr::new(&archive_cli_s),
            OsStr::new(&src_dir_s),
            OsStr::new("-method"),
            OsStr::new("3"),
            OsStr::new("-threads"),
            OsStr::new("2"),
        ],
    );

    zpaq_add(&archive_rust_s, &[&src_dir_s], "3", 2).expect("rust append");

    let size_cli_second = fs::metadata(&archive_cli)
        .expect("stat cli archive 2")
        .len();
    let size_rust_second = fs::metadata(&archive_rust)
        .expect("stat rust archive 2")
        .len();
    assert_eq!(
        size_cli_second, size_rust_second,
        "append archive size differs"
    );

    let list_out = zpaq_list(&archive_rust_s, &[]).expect("rust list");
    let list_text = format!("{}{}", list_out.stdout, list_out.stderr);
    assert!(list_text.contains("alpha.txt"), "list missing alpha.txt");
    assert!(list_text.contains("beta.txt"), "list missing beta.txt");
    assert!(list_text.contains("gamma.txt"), "list missing gamma.txt");

    let rust_extract_dir = temp.join("extract_rust");
    let cli_extract_dir = temp.join("extract_cli");
    fs::create_dir_all(&rust_extract_dir).expect("create rust extract dir");
    fs::create_dir_all(&cli_extract_dir).expect("create cli extract dir");

    let rust_extract_s = rust_extract_dir.to_string_lossy().to_string();
    zpaq_command(&["extract", &archive_cli_s, "-to", &rust_extract_s])
        .expect("rust extract cli archive");

    run_ok(
        &zpaq_bin,
        [
            OsStr::new("extract"),
            OsStr::new(&archive_rust_s),
            OsStr::new("-to"),
            cli_extract_dir.as_os_str(),
        ],
    );

    for (name, expected) in [
        ("alpha.txt", b"alpha alpha alpha\n".as_slice()),
        ("beta.txt", b"beta beta beta\n".as_slice()),
        ("gamma.txt", b"gamma append payload\n".as_slice()),
    ] {
        let rust_p = find_file_named(&rust_extract_dir, name).expect("find file in rust extract");
        let cli_p = find_file_named(&cli_extract_dir, name).expect("find file in cli extract");
        assert_eq!(
            fs::read(&rust_p).expect("read rust extracted"),
            expected,
            "rust extracted file contents differ for {name}"
        );
        assert_eq!(
            fs::read(&cli_p).expect("read cli extracted"),
            expected,
            "cli extracted file contents differ for {name}"
        );
    }

    let _ = fs::remove_dir_all(temp);
}
