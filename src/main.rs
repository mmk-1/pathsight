use clap::{Parser, Subcommand};
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
            println!("pid  {pid}");
            println!("path {}", path.display());
        }
    }
}
