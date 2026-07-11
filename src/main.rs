use clap::{Parser, Subcommand};
use std::fs::File;
use std::io;
use std::path::PathBuf;
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
            if pid == 0 {
                eprintln!("psight: pid must be greater than 0");
                process::exit(1);
            }
            let _proc = match open_proc(pid) {
                Ok(f) => f,
                Err(msg) => {
                    eprintln!("psight: {msg}");
                    process::exit(1);
                }
            };
            println!("pid  {pid}");
            println!("path {}", path.display());
        }
    }
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
