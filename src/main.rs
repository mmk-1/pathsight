mod mountinfo;
mod path_resolution;
mod report;

use clap::{ArgAction, Parser, Subcommand};
use mountinfo::{find_mount, parse_mountinfo, parse_overlay_dirs};
use path_resolution::{resolve_path, ResolveError};
use report::{
    format_inspect_json, format_inspect_text, InspectResult, MapEntry, OverlayInfo,
    PathDetails, PathOutcome,
};
use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::process;

#[derive(Parser)]
#[command(name = "psight", about = "explain what a path means for a given process")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// show what PATH means for a process
    Inspect {
        /// host process id
        #[arg(short = 'p', long = "pid", value_name = "PID")]
        pid: u32,
        /// path as seen by that process
        path: PathBuf,
        /// json instead of text
        #[arg(long)]
        json: bool,
    },
    /// inspect the same path for two processes
    Diff {
        /// host process ids
        #[arg(
            short = 'p',
            long = "pid",
            value_name = "PID",
            required = true,
            action = ArgAction::Append
        )]
        pids: Vec<u32>,
        /// path as seen by both processes
        path: PathBuf,
    },
}

#[derive(Debug, PartialEq, Eq)]
struct Creds {
    uid: [u32; 4],
    gid: [u32; 4],
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Inspect { pid, path, json } => {
            if let Err(msg) = run_inspect(pid, &path, json) {
                eprintln!("psight: {msg}");
                process::exit(1);
            }
        }
        Commands::Diff { pids, path } => {
            if let Err(msg) = run_diff(&pids, &path) {
                eprintln!("psight: {msg}");
                process::exit(1);
            }
        }
    }
}

fn run_inspect(pid: u32, path: &Path, json: bool) -> Result<(), String> {
    let result = inspect(pid, path)?;
    if json {
        println!("{}", format_inspect_json(&result));
    } else {
        print!("{}", format_inspect_text(&result));
    }
    Ok(())
}

fn run_diff(pids: &[u32], path: &Path) -> Result<(), String> {
    let [first_pid, second_pid] = pids else {
        return Err(format!(
            "diff needs exactly two --pid values, got {}",
            pids.len()
        ));
    };

    let first = inspect(*first_pid, path)?;
    let second = inspect(*second_pid, path)?;

    print!("first:\n{}\nsecond:\n{}", format_inspect_text(&first), format_inspect_text(&second));
    Ok(())
}

fn inspect(pid: u32, path: &Path) -> Result<InspectResult, String> {
    if pid == 0 {
        return Err("pid must be greater than 0".into());
    }

    let _proc = open_proc(pid)?;
    let (root_file, root_path) = open_proc_root(pid)?;
    let cwd_file = open_proc_cwd(pid)?;
    let cwd = read_proc_link(pid, "cwd")?;
    let ns_mnt = read_proc_link(pid, "ns/mnt")?;
    let ns_user = read_proc_link(pid, "ns/user")?;
    let creds = parse_status_ids(&read_proc_file(pid, "status")?)?;
    let uid_map = parse_id_map(&read_proc_file(pid, "uid_map")?)?;
    let gid_map = parse_id_map(&read_proc_file(pid, "gid_map")?)?;
    let mounts = parse_mountinfo(&read_proc_file(pid, "mountinfo")?)?;

    let base = |outcome: PathOutcome| InspectResult {
        pid,
        path: path.to_path_buf(),
        root: root_path.clone(),
        cwd: cwd.clone(),
        ns_mnt: ns_mnt.clone(),
        ns_user: ns_user.clone(),
        uid: creds.uid,
        gid: creds.gid,
        uid_map: uid_map.clone(),
        gid_map: gid_map.clone(),
        outcome,
    };

    let resolved = match resolve_path(&root_file, &cwd_file, path) {
        Ok(r) => r,
        Err(ResolveError::Missing) => return Ok(base(PathOutcome::Missing)),
        Err(ResolveError::AccessDenied) => return Ok(base(PathOutcome::AccessDenied)),
        Err(ResolveError::Other(msg)) => return Err(msg),
    };

    let covering = find_mount(&mounts, resolved.mount_id).ok_or_else(|| {
        format!(
            "mount id {} from path not found in mountinfo (mounts may have changed)",
            resolved.mount_id
        )
    })?;

    // keep root/cwd fds open until resolve finishes
    let _root = root_file;
    let _cwd = cwd_file;

    let overlay = if covering.fstype == "overlay" {
        let dirs = parse_overlay_dirs(&covering.super_options);
        Some(OverlayInfo {
            lowerdir: dirs.lowerdir,
            upperdir: dirs.upperdir,
            workdir: dirs.workdir,
        })
    } else {
        None
    };

    let mapped_uid = map_id_into_ns(&uid_map, resolved.uid);
    let mapped_gid = map_id_into_ns(&gid_map, resolved.gid);

    Ok(base(PathOutcome::Resolved(PathDetails {
        inode: resolved.inode,
        dev_major: resolved.dev_major,
        dev_minor: resolved.dev_minor,
        disk_uid: resolved.uid,
        disk_gid: resolved.gid,
        mapped_uid,
        mapped_gid,
        mount_id: covering.id,
        mount_fstype: covering.fstype.clone(),
        mount_target: covering.target.clone(),
        mount_bind: covering.root.clone(),
        mount_flags: covering.options.clone(),
        overlay,
    })))
}

fn open_proc(pid: u32) -> Result<File, String> {
    match File::open(format!("/proc/{pid}")) {
        Ok(f) => Ok(f),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            Err(format!("no such process {pid}"))
        }
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            Err(format!("permission denied reading /proc/{pid}"))
        }
        Err(e) => Err(format!("cannot open /proc/{pid}: {e}")),
    }
}

// open root so the kernel follows it; do not readlink /proc/<pid>/root
fn open_proc_root(pid: u32) -> Result<(File, PathBuf), String> {
    let file = match File::open(format!("/proc/{pid}/root")) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Err(format!("no such process {pid}"));
        }
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            return Err(format!("permission denied reading /proc/{pid}/root"));
        }
        Err(e) => return Err(format!("cannot open /proc/{pid}/root: {e}")),
    };

    let path = std::fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd())).map_err(|e| {
        format!("cannot resolve opened /proc/{pid}/root: {e}")
    })?;

    Ok((file, path))
}

fn open_proc_cwd(pid: u32) -> Result<File, String> {
    match File::open(format!("/proc/{pid}/cwd")) {
        Ok(f) => Ok(f),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            Err(format!("no such process {pid}"))
        }
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            Err(format!("permission denied reading /proc/{pid}/cwd"))
        }
        Err(e) => Err(format!("cannot open /proc/{pid}/cwd: {e}")),
    }
}

fn read_proc_link(pid: u32, name: &str) -> Result<PathBuf, String> {
    let link = format!("/proc/{pid}/{name}");
    match std::fs::read_link(&link) {
        Ok(p) => Ok(p),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            Err(format!("no such process {pid}"))
        }
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            Err(format!("permission denied reading {link}"))
        }
        Err(e) => Err(format!("cannot read {link}: {e}")),
    }
}

fn read_proc_file(pid: u32, name: &str) -> Result<String, String> {
    let path = format!("/proc/{pid}/{name}");
    match std::fs::read_to_string(&path) {
        Ok(s) => Ok(s),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            Err(format!("no such process {pid}"))
        }
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            Err(format!("permission denied reading {path}"))
        }
        Err(e) => Err(format!("cannot read {path}: {e}")),
    }
}

fn parse_status_ids(text: &str) -> Result<Creds, String> {
    let mut uid = None;
    let mut gid = None;

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("Uid:") {
            uid = Some(parse_status_id_line("Uid", rest)?);
        } else if let Some(rest) = line.strip_prefix("Gid:") {
            gid = Some(parse_status_id_line("Gid", rest)?);
        }
    }

    match (uid, gid) {
        (Some(uid), Some(gid)) => Ok(Creds { uid, gid }),
        (None, _) => Err("status missing Uid line".into()),
        (_, None) => Err("status missing Gid line".into()),
    }
}

fn parse_status_id_line(label: &str, rest: &str) -> Result<[u32; 4], String> {
    let nums: Vec<&str> = rest.split_whitespace().collect();
    if nums.len() != 4 {
        return Err(format!("{label} line needs 4 fields, got {}", nums.len()));
    }
    let mut out = [0u32; 4];
    for (i, s) in nums.iter().enumerate() {
        out[i] = s
            .parse()
            .map_err(|_| format!("bad {label} value `{s}`"))?;
    }
    Ok(out)
}

fn parse_id_map(text: &str) -> Result<Vec<MapEntry>, String> {
    let mut entries = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() != 3 {
            return Err(format!(
                "id map line {}: expected 3 fields, got {}",
                i + 1,
                parts.len()
            ));
        }
        let first = parts[0]
            .parse()
            .map_err(|_| format!("id map line {}: bad first id `{}`", i + 1, parts[0]))?;
        let lower_first = parts[1]
            .parse()
            .map_err(|_| format!("id map line {}: bad lower id `{}`", i + 1, parts[1]))?;
        let count = parts[2]
            .parse()
            .map_err(|_| format!("id map line {}: bad count `{}`", i + 1, parts[2]))?;
        entries.push(MapEntry {
            first,
            lower_first,
            count,
        });
    }
    Ok(entries)
}

// map a host/parent-ns id into the process user namespace via uid_map/gid_map
fn map_id_into_ns(map: &[MapEntry], id: u32) -> Option<u32> {
    for e in map {
        let start = u64::from(e.lower_first);
        let Some(end) = start.checked_add(u64::from(e.count)) else {
            continue;
        };
        let id = u64::from(id);
        if id >= start && id < end {
            return Some(e.first + (id as u32 - e.lower_first));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_identity_uid_map() {
        let map = parse_id_map("         0          0 4294967295\n").unwrap();
        assert_eq!(
            map,
            vec![MapEntry {
                first: 0,
                lower_first: 0,
                count: 4294967295,
            }]
        );
    }

    #[test]
    fn parse_rootless_uid_map() {
        let fixture = "\
         0     100000      65536
     65536     165536      65536
";
        let map = parse_id_map(fixture).unwrap();
        assert_eq!(
            map,
            vec![
                MapEntry {
                    first: 0,
                    lower_first: 100000,
                    count: 65536,
                },
                MapEntry {
                    first: 65536,
                    lower_first: 165536,
                    count: 65536,
                },
            ]
        );
    }

    #[test]
    fn parse_empty_uid_map() {
        assert_eq!(parse_id_map("").unwrap(), vec![]);
        assert_eq!(parse_id_map("\n\n").unwrap(), vec![]);
    }

    #[test]
    fn parse_id_map_rejects_bad_line() {
        assert!(parse_id_map("0 0\n").is_err());
        assert!(parse_id_map("0 x 1\n").is_err());
    }

    #[test]
    fn parse_status_uid_gid() {
        let fixture = "\
Name:\tpsight
Umask:\t0022
State:\tR (running)
Uid:\t1000\t1000\t1000\t1000
Gid:\t100\t100\t100\t100
";
        let creds = parse_status_ids(fixture).unwrap();
        assert_eq!(creds.uid, [1000, 1000, 1000, 1000]);
        assert_eq!(creds.gid, [100, 100, 100, 100]);
    }

    #[test]
    fn map_id_into_ns_identity() {
        let map = parse_id_map("0 0 4294967295\n").unwrap();
        assert_eq!(map_id_into_ns(&map, 0), Some(0));
        assert_eq!(map_id_into_ns(&map, 1000), Some(1000));
    }

    #[test]
    fn map_id_into_ns_rootless() {
        let map = parse_id_map("0 100000 65536\n").unwrap();
        assert_eq!(map_id_into_ns(&map, 100000), Some(0));
        assert_eq!(map_id_into_ns(&map, 100001), Some(1));
        assert_eq!(map_id_into_ns(&map, 99999), None);
        assert_eq!(map_id_into_ns(&map, 165536), None);
    }
}
