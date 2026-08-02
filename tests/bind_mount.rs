//! unshare + bind mount: inspect must report the bind source inode.
//! skips if user namespaces / unshare / mount are unavailable.

use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "pathsight-bind-{}-{}",
            std::process::id(),
            nanos
        ));
        fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn parse_inode_line(stdout: &str) -> Option<u64> {
    for line in stdout.lines() {
        let Some(rest) = line.strip_prefix("inode") else {
            continue;
        };
        let ino = rest.split_whitespace().next()?;
        return ino.parse().ok();
    }
    None
}

#[test]
fn unshare_bind_mount_known_inode() {
    let dir = TempDir::new();
    let host = dir.path().join("host");
    let other = dir.path().join("other");
    fs::write(&host, b"HOST\n").expect("write host file");
    fs::write(&other, b"OTHER\n").expect("write other file");

    let host_ino = fs::metadata(&host).expect("stat host").ino();
    let other_ino = fs::metadata(&other).expect("stat other").ino();
    assert_ne!(host_ino, other_ino, "test files must be distinct inodes");

    let script = format!(
        "mount --bind '{}' '{}' && printf '%s\\n' \"$$\" && exec sleep 60",
        other.display(),
        host.display()
    );

    let mut child = match Command::new("unshare")
        .args(["--user", "--map-root-user", "--mount", "sh", "-c", &script])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => ChildGuard(c),
        Err(e) => {
            eprintln!("skipping unshare bind-mount test: cannot run unshare: {e}");
            return;
        }
    };

    let stdout = child.0.stdout.take().expect("child stdout");
    let mut reader = BufReader::new(stdout);
    let mut pid_line = String::new();
    if reader.read_line(&mut pid_line).ok().unwrap_or(0) == 0 {
        let mut err = String::new();
        if let Some(mut stderr) = child.0.stderr.take() {
            let _ = stderr.read_to_string(&mut err);
        }
        let status = child.0.wait().ok();
        eprintln!(
            "skipping unshare bind-mount test: unshare/mount failed (status={status:?}, stderr={})",
            err.trim()
        );
        return;
    }
    drop(reader);

    let pid: u32 = match pid_line.trim().parse() {
        Ok(p) if p > 0 => p,
        _ => {
            eprintln!(
                "skipping unshare bind-mount test: bad pid line {:?}",
                pid_line.trim()
            );
            return;
        }
    };

    let path = host.to_str().expect("utf-8 path");
    let output = Command::new(env!("CARGO_BIN_EXE_psight"))
        .args(["inspect", "-p", &pid.to_string(), path])
        .output()
        .expect("run psight");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "psight inspect failed: status={} stdout={} stderr={}",
        output.status,
        stdout,
        stderr
    );

    let got = parse_inode_line(&stdout).unwrap_or_else(|| {
        panic!("inode line missing from psight output\nstdout:\n{stdout}\nstderr:\n{stderr}");
    });
    assert_eq!(
        got, other_ino,
        "inspect should see the bind source inode, not the covered host file\nstdout:\n{stdout}"
    );
    assert_ne!(got, host_ino);
}
