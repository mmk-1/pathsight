mod mountinfo;
mod path_resolution;

use clap::{Parser, Subcommand};
use mountinfo::{find_mount, parse_mountinfo};
use path_resolution::resolve_path;
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
    },
}

#[derive(Debug, PartialEq, Eq)]
struct Creds {
    uid: [u32; 4],
    gid: [u32; 4],
}

#[derive(Debug, PartialEq, Eq)]
struct IdMapEntry {
    first: u32,
    lower_first: u32,
    count: u32,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Inspect { pid, path } => {
            if let Err(msg) = run_inspect(pid, &path) {
                eprintln!("psight: {msg}");
                process::exit(1);
            }
        }
    }
}

fn run_inspect(pid: u32, path: &Path) -> Result<(), String> {
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
    let resolved = resolve_path(&root_file, &cwd_file, path)?;
    let covering = find_mount(&mounts, resolved.mount_id).ok_or_else(|| {
        format!(
            "mount id {} from path not found in mountinfo (mounts may have changed)",
            resolved.mount_id
        )
    })?;

    // keep root/cwd fds open for later path walks
    let _root = root_file;
    let _cwd = cwd_file;

    println!("pid     {pid}");
    println!("path    {}", path.display());
    println!("root    {}", root_path.display());
    println!("cwd     {}", cwd.display());
    println!("ns.mnt  {}", ns_mnt.display());
    println!("ns.user {}", ns_user.display());
    println!(
        "uid     r={} e={} s={} fs={}",
        creds.uid[0], creds.uid[1], creds.uid[2], creds.uid[3]
    );
    println!(
        "gid     r={} e={} s={} fs={}",
        creds.gid[0], creds.gid[1], creds.gid[2], creds.gid[3]
    );
    print_id_map("uid.map", &uid_map);
    print_id_map("gid.map", &gid_map);
    println!("mounts  {}", mounts.len());
    println!(
        "inode   {}  dev {}:{}",
        resolved.inode, resolved.dev_major, resolved.dev_minor
    );
    println!(
        "mount   id={}  {}  {}",
        covering.id, covering.fstype, covering.target
    );
    println!("        bind   {}", covering.root);
    println!("        flags  {}", covering.options);
    Ok(())
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

fn parse_id_map(text: &str) -> Result<Vec<IdMapEntry>, String> {
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
        entries.push(IdMapEntry {
            first,
            lower_first,
            count,
        });
    }
    Ok(entries)
}

fn print_id_map(label: &str, entries: &[IdMapEntry]) {
    if entries.is_empty() {
        println!("{label} (empty)");
        return;
    }
    for e in entries {
        println!(
            "{label} {} {} {}",
            e.first, e.lower_first, e.count
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_identity_uid_map() {
        let map = parse_id_map("         0          0 4294967295\n").unwrap();
        assert_eq!(
            map,
            vec![IdMapEntry {
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
                IdMapEntry {
                    first: 0,
                    lower_first: 100000,
                    count: 65536,
                },
                IdMapEntry {
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
}
