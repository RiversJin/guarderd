use anyhow::{Context, Result, bail};
use chrono::Utc;

use clap::{Args, Parser, Subcommand};
use fs2::FileExt;
use nix::{
    libc::{self, prctl}, sys::signal::kill, unistd::{self, dup2_stderr, dup2_stdout, ForkResult, Pid}
};
use std::{
    fs::{File, OpenOptions}, io::{Read, Write}, os::unix::process::CommandExt, path::PathBuf, process::{exit, Command}, sync::{atomic::AtomicBool, Arc, Mutex}, thread, time::Duration
};

const STATUS_PATH: &str = "guarderd.status.d";
const DEFAULT_MAX_LOG_SIZE_MIB: u64 = 10;

fn daemonize() -> Result<Pid> {
    if let ForkResult::Parent { .. } = unsafe { unistd::fork()? } {
        std::process::exit(0);
    }

    unistd::setsid()?;

    if let ForkResult::Parent { .. } = unsafe { unistd::fork()? } {
        std::process::exit(0);
    }

    unistd::chdir("/")?;
    Ok(unistd::getpid())
}

fn is_process_exist(pid: impl Into<Pid>) -> bool {
    let pid = pid.into();
    match kill(pid, None) {
        Ok(_) => true,
        Err(nix::errno::Errno::ESRCH) => false,
        Err(_) => true,
    }
}

#[derive(Debug)]
struct Daemon {
    pid_file: PathBuf,
    child_pid: Arc<Mutex<Option<Pid>>>,
    log_path: PathBuf,
    log_file: Arc<Mutex<Option<File>>>,
    lock_file: PathBuf,
    lock_handle: Option<File>,
    running: Arc<AtomicBool>,
}

impl Daemon {
    fn new() -> Result<Self> {
        let current_dir = std::env::current_dir().context("fail to current dir")?;
        let status_dir = current_dir.join(STATUS_PATH);
        std::fs::create_dir_all(&status_dir).context("fail to create status dir")?;

        let pid_file = status_dir.join("pid");
        let lock_file = status_dir.join("lock");
        let log_path = status_dir.join("stdout.log");

        Ok(Daemon {
            pid_file,
            child_pid: Arc::new(None.into()),
            log_path,
            lock_file,
            log_file: Arc::new(Mutex::new(None)),
            lock_handle: None,
            running: Arc::new(AtomicBool::new(false)),
        })
    }

    fn try_lock(&mut self) -> Result<()> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&self.lock_file)
            .with_context(|| format!("failed to open lock file: {}", self.lock_file.display()))?;

        file.try_lock_exclusive()
            .context("failed to acquire lock, the daemon may already be running")?;

        self.lock_handle = Some(file);

        Ok(())
    }

    fn save_pids(&self, daemon_pid: Pid, child_pid: Pid) -> Result<()> {
        let content = format!(
            "daemon_pid: {}\nchild_pid: {}\n",
            daemon_pid.as_raw(),
            child_pid.as_raw()
        );
        std::fs::write(&self.pid_file, content).context("failed to write PID file")?;
        Ok(())
    }

    fn get_pids(&self) -> Result<(Pid, Pid)> {
        if !self.pid_file.exists() {
            bail!("PID file does not exist: {}", self.pid_file.display());
        }

        let content = std::fs::read_to_string(&self.pid_file).context("failed to read PID file")?;

        let mut daemon_pid: Option<i32> = None;
        let mut child_pid: Option<i32> = None;

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            if let Some((key, value)) = line.split_once(':') {
                let key = key.trim();
                let value = value.trim();
                
                match key {
                    "daemon_pid" => {
                        daemon_pid = Some(value.parse::<i32>()
                            .with_context(|| format!("failed to parse daemon_pid: {}", value))?);
                    }
                    "child_pid" => {
                        child_pid = Some(value.parse::<i32>()
                            .with_context(|| format!("failed to parse child_pid: {}", value))?);
                    }
                    _ => {
                        // Ignore unknown keys for forward compatibility
                    }
                }
            }
        }

        let daemon_pid = daemon_pid.ok_or_else(|| anyhow::anyhow!("daemon_pid not found in PID file"))?;
        let child_pid = child_pid.ok_or_else(|| anyhow::anyhow!("child_pid not found in PID file"))?;

        Ok((Pid::from_raw(daemon_pid), Pid::from_raw(child_pid)))
    }

    fn stop(&self) -> Result<()> {
        let (daemon_pid, child_pid) = self.get_pids()?;
        if !is_process_exist(daemon_pid) {
            println!("Daemon {} is not running", daemon_pid);
            return Ok(());
        }

        kill(daemon_pid, nix::sys::signal::Signal::SIGTERM)
            .with_context(|| format!("failed to send SIGTERM to daemon {}", daemon_pid))?;

        std::thread::sleep(std::time::Duration::from_millis(1000));

        if is_process_exist(daemon_pid) {
            println!(
                "Daemon {} is still running after SIGTERM, sending SIGKILL",
                daemon_pid
            );
            kill(daemon_pid, nix::sys::signal::Signal::SIGKILL)
                .with_context(|| format!("failed to send SIGKILL to daemon {}", daemon_pid))?;
        }

        // wait for child exit, with 5 seconds timeout
        // if the child process is still running after 5 seconds, kill it with SIGKILL
        println!(
            "Stopped daemon {}, waiting for child {} to exit",
            daemon_pid, child_pid
        );
        let start = std::time::Instant::now();
        while start.elapsed().as_secs() < 5 {
            if !is_process_exist(child_pid) {
                println!("Child process {} exited", child_pid);
                break;
            }
            println!("Child process {} is still running", child_pid);
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        if is_process_exist(child_pid) {
            println!(
                "Child process {} is still running after 5 seconds, killing it",
                child_pid
            );
            kill(child_pid, nix::sys::signal::Signal::SIGKILL)
                .with_context(|| format!("failed to send SIGKILL to child {}", child_pid))?;
        }

        Ok(())
    }

    fn start(&mut self, command: Vec<String>, restart_interval: Duration, max_log_size: u64) {
        if let Err(err) = self.try_lock() {
            println!(
                "Failed to acquire lock: {}, may be another instance is running",
                err
            );
            return;
        }

        let daemon_pid = daemonize().expect("Failed to daemonize");

        self.running
            .store(true, std::sync::atomic::Ordering::SeqCst);

        let (read_pipe, write_pipe) = std::io::pipe().expect("Failed to create pipe");

        self.setup_signal_handler();
        self.spawn_log_thread(read_pipe, max_log_size);
        dup2_stdout(&write_pipe).expect("Failed to redirect stdout");
        dup2_stderr(&write_pipe).expect("Failed to redirect stderr");

        while self.running.load(std::sync::atomic::Ordering::SeqCst) {
            let mut child = unsafe {
                Command::new(command[0].clone())
                    .args(&command[1..])
                    .stdout(std::process::Stdio::inherit())
                    .stderr(std::process::Stdio::inherit())
                    .pre_exec(|| {
                        prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                         std::io::Result::Ok(())
                    })
                    .spawn()
            }.expect("Failed to spawn child process");

            let child_pid = Pid::from_raw(child.id() as i32);
            self.child_pid.lock().unwrap().replace(child_pid);
            self.save_pids(daemon_pid, child_pid).expect("Failed to save PIDs");

            let status = child.wait().expect("Failed to wait for child process");

            println!(
                "[{}] Child process {} exited with status {}",
                Utc::now().to_rfc3339(),
                child_pid,
                status
            );

            if self.running.load(std::sync::atomic::Ordering::SeqCst) {
                println!(
                    "[{}] Restarting child process in {} seconds...",
                    Utc::now().to_rfc3339(),
                    restart_interval.as_secs()
                );

                for _ in 0..restart_interval.as_secs() {
                    if !self.running.load(std::sync::atomic::Ordering::SeqCst) {
                        break;
                    }
                    thread::sleep(Duration::from_secs(1));
                }
            }
        }

    }

    fn spawn_log_thread(&self, reader: impl Read + Send + 'static, max_log_size: u64) -> thread::JoinHandle<()> {
        let running = self.running.clone();
        let log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
            .expect("Failed to open log file");

        self.log_file.lock().unwrap().replace(log_file.try_clone().expect("Failed to clone log file handle"));

        thread::spawn(move || {
            let mut reader = reader;
            let mut buf = [0; 4096];
            let written_check = 1u64 << 20; // 1 MB
            let mut bytes_written = 0;
            let mut log_file = log_file;

            while running.load(std::sync::atomic::Ordering::Relaxed) {
                match reader.read(&mut buf) {
                    Ok(n) if n > 0 => {
                        if bytes_written >= written_check {
                            bytes_written = 0;
                            if log_file
                                .metadata()
                                .expect("Failed to get log file metadata")
                                .len()
                                > max_log_size
                            {
                                log_file.set_len(0).expect("Failed to truncate log file");
                                let msg = format!(
                                    "[{}] Log size exceeded. Rotated",
                                    Utc::now().to_rfc3339()
                                );
                                log_file
                                    .write_all(msg.as_bytes())
                                    .expect("Failed to write to log file");
                            }
                            log_file.flush().expect("Failed to flush log file");
                        }

                        log_file
                            .write_all(&buf[..n])
                            .expect("Failed to write to log file");
                        bytes_written += n as u64;
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => {
                        break;
                    }
                    Err(err) => {
                        eprintln!("Failed to read from pipe: {}", err);
                        break;
                    }
                    Ok(_) => {
                        // EOF
                        break;
                    }
                }
            }
        })
    }

    fn setup_signal_handler(&self) {
        let running = self.running.clone();
        let child_pid = self.child_pid.clone();
        let log_file = self.log_file.clone();
        ctrlc::set_handler(move || {
            if let Some(pid) = child_pid.lock().unwrap().as_ref() {
                _ = kill(*pid, nix::sys::signal::Signal::SIGTERM);
            }
            running.store(false, std::sync::atomic::Ordering::Relaxed);
            println!("[{}] Daemon: Received Ctrl-C, shutting down...", Utc::now().to_rfc3339());
            log_file.lock().unwrap().as_mut().map(|f| f.sync_all().expect("Failed to sync log file"));
            exit(0);

        })
        .expect("Failed to set Ctrl-C handler");
    }

    fn status(&self) {
        let (daemon_pid, child_pid) = self.get_pids().expect("Failed to get PIDs");
        let is_child_running = is_process_exist(child_pid);
        let is_daemon_running = is_process_exist(daemon_pid);
        println!("Daemon PID: {}, running: {}", daemon_pid, is_daemon_running);
        println!("Child PID: {}, running: {}", child_pid, is_child_running);
    }

}

#[derive(Parser, Debug)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}


#[derive(Subcommand, Debug)]
enum Commands {
    /// Start a new guard
    Start(StartArgs),
    /// Stop the guard
    Stop,
    /// Show the status of the guard
    Status,
}


#[derive(Args, Debug)]
struct StartArgs {
    /// The interval (in seconds) to restart the guard
    #[arg(long, default_value_t = 5)]
    restart_interval: u64,

    /// The command to run
    #[arg(required = true, last = true)]
    command: Vec<String>,

    /// The maximum size of the log file (in MiB)
    #[arg(long, default_value_t = DEFAULT_MAX_LOG_SIZE_MIB)]
    max_log_size_mib: u64,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut daemon = Daemon::new()?;
    match cli.command {
        Commands::Start(args) => {
            daemon.start(args.command, Duration::from_secs(args.restart_interval), args.max_log_size_mib);
        }
        Commands::Stop => {
            daemon.stop()?;
        }
        Commands::Status => {
            daemon.status();
        }
    }
    Ok(())
}
