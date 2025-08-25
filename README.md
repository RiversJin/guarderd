# Gurarderd

A lightweight process guard daemon written in Rust that monitors and automatically restarts processes when they exit or crash.

## Features

- **Process Monitoring**: Continuously monitors a specified process and automatically restarts it if it exits
- **Daemon Mode**: Runs as a background daemon process
- **Configurable Restart Interval**: Set custom restart intervals between process restarts
- **Log Management**: Captures stdout/stderr from monitored processes with automatic log rotation
- **Process Control**: Start, stop, and check status of the guard daemon
- **Signal Handling**: Graceful shutdown on SIGTERM/SIGKILL
- **Lock File Protection**: Prevents multiple daemon instances from running simultaneously

## Installation

### From Source

```bash
git clone https://github.com/RiversJin/gurarderd.git
cd gurarderd
cargo build --release
```

The binary will be available at `target/release/gurarderd`.

### Recommended: Using Zig Build

For better cross-platform compatibility and easier deployment across different machines, it's recommended to use zig-build:

```bash
# Install zig if you haven't already
# Then use cargo with zig as the linker
cargo zigbuild --release
```

This approach produces binaries that are more portable and can run on different systems without compatibility issues.

## Usage

### Start Monitoring a Process

```bash
gurarderd start [OPTIONS] -- <COMMAND>
```

**Options:**
- `--restart-interval <SECONDS>`: Set restart interval in seconds (default: 5)
- `--max-log-size-mib <MIB>`: Maximum log file size in MiB (default: 10)

**Examples:**

```bash
# Monitor a simple command with default settings
gurarderd start -- python my_script.py

# Monitor with custom restart interval
gurarderd start --restart-interval 10 -- ./my_application

# Monitor with custom restart interval and log size
gurarderd start --restart-interval 30 --max-log-size-mib 50 -- node server.js
```

### Check Daemon Status

```bash
gurarderd status
```

This will show the daemon PID, child process PID, and their running status.

### Stop the Daemon

```bash
gurarderd stop
```

This will gracefully stop the daemon and the monitored process.

## How It Works

1. **Daemon Creation**: When started, gurarderd forks itself into a background daemon process
2. **Process Monitoring**: The daemon spawns and monitors the specified command
3. **Automatic Restart**: If the monitored process exits, the daemon waits for the configured interval and restarts it
4. **Log Capture**: All stdout/stderr from the monitored process is captured and written to `guarderd.status.d/stdout.log`
5. **Log Rotation**: When the log file exceeds the maximum size, it's automatically truncated
6. **Status Tracking**: Process IDs and status information are stored in `guarderd.status.d/`

## File Structure

When running, gurarderd creates a `guarderd.status.d/` directory containing:

- `pid`: Contains daemon and child process PIDs
- `lock`: Lock file to prevent multiple daemon instances
- `stdout.log`: Captured output from the monitored process

## Requirements

- Linux
- Rust 1.70+ (for building from source)
