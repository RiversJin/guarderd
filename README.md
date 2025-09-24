# Guarderd

A lightweight process guard daemon written in Rust that monitors and automatically restarts processes when they exit or crash.

## Features

- **Process Monitoring**: Continuously monitors a specified process and automatically restarts it if it exits
- **Daemon Mode**: Runs as a background daemon process
- **Configurable Restart Interval**: Set custom restart intervals between process restarts
- **Grace Period Validation**: Ensures the monitored process starts successfully before considering it stable
- **Log Management**: Captures stdout/stderr from monitored processes with automatic log rotation
- **Process Control**: Start, stop, and check status of the guard daemon
- **Signal Handling**: Graceful shutdown on SIGTERM/SIGKILL
- **Lock File Protection**: Prevents multiple daemon instances from running simultaneously

## Installation

### From Source

```bash
git clone https://github.com/RiversJin/guarderd.git
cd guarderd
cargo build --release
```

The binary will be available at `target/release/guarderd`.

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
guarderd start [OPTIONS] -- <COMMAND>
```

**Options:**
- `--restart-interval <SECONDS>`: Set restart interval in seconds (default: 5)
- `--max-log-size-mib <MIB>`: Maximum log file size in MiB (default: 10)
- `--grace-period <SECONDS>`: Grace period in seconds to consider the child process started successfully (default: 5)

**Examples:**

```bash
# Monitor a simple command with default settings
guarderd start -- python my_script.py

# Monitor with custom restart interval
guarderd start --restart-interval 10 -- ./my_application

# Monitor with custom restart interval and log size
guarderd start --restart-interval 30 --max-log-size-mib 50 -- node server.js

# Monitor with custom grace period (wait 10 seconds to confirm successful startup)
guarderd start --grace-period 10 -- ./slow_startup_app

# Monitor with all custom parameters
guarderd start --restart-interval 15 --max-log-size-mib 20 --grace-period 30 -- python my_service.py
```

### Check Daemon Status

```bash
guarderd status
```

This will show the daemon PID, child process PID, and their running status.

### Stop the Daemon

```bash
guarderd stop
```

This will gracefully stop the daemon and the monitored process.

## How It Works

1. **Daemon Creation**: When started, guarderd forks itself into a background daemon process
2. **Grace Period Check**: The daemon monitors the child process during a configurable grace period to ensure successful startup
3. **Process Monitoring**: After the grace period, the daemon continuously monitors the specified command
4. **Automatic Restart**: If the monitored process exits, the daemon waits for the configured interval and restarts it
5. **Log Capture**: All stdout/stderr from the monitored process is captured and written to `guarderd.status.d/stdout.log`
6. **Log Rotation**: When the log file exceeds the maximum size, it's automatically truncated
7. **Status Tracking**: Process IDs and status information are stored in `guarderd.status.d/`

## File Structure

When running, guarderd creates a `guarderd.status.d/` directory containing:

- `pid`: Contains daemon and child process PIDs
- `lock`: Lock file to prevent multiple daemon instances
- `stdout.log`: Captured output from the monitored process

## Requirements

- Linux
- Rust 1.70+ (for building from source)
