# pathsight

Linux CLI (`psight`): what a path means for a given process (mount ns, root, cwd, inode, covering mount).

```bash
cargo build
./target/debug/psight inspect -p PID PATH
```

Other PIDs often need root. Kernel needs `openat2` / `statx` mount id (roughly 5.8+).

## Done

- `inspect -p PID PATH` — process context, resolve path, covering mount
- some unit tests for parsing and a integration test: `unshare` bind mount → known inode (`tests/bind_mount.rs`)

## Layout

- `src/main.rs` — CLI, `/proc` reads, inspect output
- `src/path_resolution.rs` — `openat2` / `statx`
- `src/mountinfo.rs` — parse `/proc/<pid>/mountinfo`
- `tests/bind_mount.rs` — real bind-mount check (skips if no userns)

## Still to do

- overlay `lowerdir` / `upperdir` / `workdir`
- disk UID vs mapped UID
- `--json`
- `diff --pid A --pid B PATH`