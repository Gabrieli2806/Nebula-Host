//! Servers still running from before the app (re)started.
//!
//! A server outlives the app that started it: an update, a crash or a forced
//! close ends the app, and the game server keeps going with nobody watching.
//! The app used to show it as off, and starting it again failed on the port —
//! or, worse, put a second process on the same world.
//!
//! On launch (and before every start) the app looks for a game process whose
//! working directory is the server's folder and takes it back: status,
//! resources and players come back, the console follows the server's log
//! file, and Stop asks it to shut down cleanly (Ctrl+C on Windows, SIGTERM
//! elsewhere — both run Minecraft's own save-and-stop). What cannot come back
//! is the server's console input: that pipe died with the old app.

use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// A game process found running in a server's folder.
#[derive(Debug, Clone, Copy)]
pub struct Found {
    pub pid: u32,
    pub started_at: DateTime<Utc>,
}

fn same_dir(a: &Path, b: &Path) -> bool {
    let a = std::fs::canonicalize(a).unwrap_or_else(|_| a.to_path_buf());
    let b = std::fs::canonicalize(b).unwrap_or_else(|_| b.to_path_buf());
    if cfg!(windows) {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

/// Every Java process with its working directory, for matching against
/// server folders. One scan serves all servers.
pub fn java_processes() -> Vec<(Found, PathBuf)> {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_cwd(UpdateKind::Always),
    );
    system
        .processes()
        .iter()
        .filter(|(_, process)| process.name().to_string_lossy().to_lowercase().starts_with("java"))
        .filter_map(|(pid, process)| {
            let cwd = process.cwd()?.to_path_buf();
            let started_at = Utc.timestamp_opt(process.start_time() as i64, 0).single()?;
            Some((Found { pid: pid.as_u32(), started_at }, cwd))
        })
        .collect()
}

/// The game process running in `dir`, if any.
pub fn find_in(processes: &[(Found, PathBuf)], dir: &Path) -> Option<Found> {
    processes.iter().find(|(_, cwd)| same_dir(cwd, dir)).map(|(found, _)| *found)
}

/// Is this exact process (same pid, same start) still alive?
pub fn is_alive(found: Found) -> bool {
    let pid = Pid::from_u32(found.pid);
    let mut system = System::new();
    system.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, ProcessRefreshKind::nothing());
    system
        .process(pid)
        .is_some_and(|process| (process.start_time() as i64 - found.started_at.timestamp()).abs() <= 2)
}

/// Ask a process we have no console pipe to to shut down cleanly.
#[cfg(windows)]
pub fn interrupt(pid: u32) -> std::io::Result<()> {
    use std::sync::Mutex;
    use windows_sys::Win32::System::Console::{
        AttachConsole, FreeConsole, GenerateConsoleCtrlEvent, SetConsoleCtrlHandler, CTRL_C_EVENT,
    };

    // Attaching to another console is process-wide: one at a time.
    static LOCK: Mutex<()> = Mutex::new(());
    let _guard = LOCK.lock();
    // SAFETY: plain Win32 calls; the app has no console of its own (it is a
    // GUI process), and it ignores Ctrl+C while attached so the event only
    // reaches the server.
    unsafe {
        FreeConsole();
        if AttachConsole(pid) == 0 {
            return Err(std::io::Error::last_os_error());
        }
        SetConsoleCtrlHandler(None, 1);
        let sent = GenerateConsoleCtrlEvent(CTRL_C_EVENT, 0);
        let error = std::io::Error::last_os_error();
        FreeConsole();
        // The event is delivered asynchronously; keep ignoring it a moment.
        std::thread::sleep(Duration::from_millis(500));
        SetConsoleCtrlHandler(None, 0);
        if sent == 0 {
            return Err(error);
        }
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn interrupt(pid: u32) -> std::io::Result<()> {
    let status = std::process::Command::new("kill").args(["-TERM", &pid.to_string()]).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!("kill exited with {status}")))
    }
}

/// Follow a log file from its current end, handing each new line to `line`.
/// Starts over when the file is replaced (the server rotates it at start).
/// Ends when `alive` says so.
pub fn tail(path: PathBuf, alive: impl Fn() -> bool, mut line: impl FnMut(String)) {
    let mut reader: Option<BufReader<File>> = None;
    let mut position: u64 = 0;
    let mut buf = String::new();
    let mut first_open = true;
    loop {
        if reader.is_none() {
            if let Ok(file) = File::open(&path) {
                let len = file.metadata().map(|m| m.len()).unwrap_or(0);
                let mut file = BufReader::new(file);
                // On first open skip what was already written: that is the
                // past, not what is happening now.
                position = if first_open { len } else { 0 };
                let _ = file.seek(SeekFrom::Start(position));
                reader = Some(file);
                first_open = false;
            }
        }
        if let Some(file) = reader.as_mut() {
            loop {
                buf.clear();
                match file.read_line(&mut buf) {
                    Ok(0) => break,
                    Ok(n) if buf.ends_with('\n') => {
                        position += n as u64;
                        line(buf.trim_end_matches(['\r', '\n']).to_string());
                    }
                    Ok(_) => {
                        // A partial line: read it again once it is complete.
                        let _ = file.seek(SeekFrom::Start(position));
                        break;
                    }
                    Err(_) => break,
                }
            }
            // Truncated or replaced: start again from the top.
            let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            if len < position {
                reader = None;
                position = 0;
            }
        }
        if !alive() {
            return;
        }
        std::thread::sleep(Duration::from_millis(400));
    }
}
