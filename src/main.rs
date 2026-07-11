use clap::{Parser, Subcommand};
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
    let cwd = read_proc_link(pid, "cwd")?;
    let ns_mnt = read_proc_link(pid, "ns/mnt")?;
    let ns_user = read_proc_link(pid, "ns/user")?;

    // keep the root fd open for later path walks
    let _root = root_file;

    println!("pid     {pid}");
    println!("path    {}", path.display());
    println!("root    {}", root_path.display());
    println!("cwd     {}", cwd.display());
    println!("ns.mnt  {}", ns_mnt.display());
    println!("ns.user {}", ns_user.display());
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
