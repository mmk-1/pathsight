use std::env;
use std::process;

struct InspectArgs {
    pid: u32,
    path: String,
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    match parse_inspect(&args) {
        Ok(a) => {
            println!("pid  {}", a.pid);
            println!("path {}", a.path);
        }
        Err(msg) => {
            eprintln!("psight: {msg}");
            process::exit(1);
        }
    }
}

fn parse_inspect(args: &[String]) -> Result<InspectArgs, String> {
    if args.is_empty() {
        return Err("expected command: inspect".into());
    }

    if args[0] != "inspect" {
        return Err(format!("unknown command '{}'", args[0]));
    }

    let mut pid: Option<u32> = None;
    let mut path: Option<String> = None;
    let mut i = 1;

    while i < args.len() {
        let a = &args[i];
        if a == "--pid" || a == "-p" {
            i += 1;
            let Some(raw) = args.get(i) else {
                return Err(format!("missing value for {a}"));
            };
            if pid.is_some() {
                return Err("pid given more than once".into());
            }
            let Ok(n) = raw.parse::<u32>() else {
                return Err(format!("invalid pid '{raw}'"));
            };
            if n == 0 {
                return Err("pid must be greater than 0".into());
            }
            pid = Some(n);
        } else if a.starts_with('-') {
            return Err(format!("unknown flag '{a}'"));
        } else if path.is_some() {
            return Err(format!("unexpected argument '{a}'"));
        } else {
            path = Some(a.clone());
        }
        i += 1;
    }

    let Some(pid) = pid else {
        return Err("missing --pid / -p".into());
    };
    let Some(path) = path else {
        return Err("missing path".into());
    };

    Ok(InspectArgs { pid, path })
}
