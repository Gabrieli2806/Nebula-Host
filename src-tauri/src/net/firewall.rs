//! Windows Firewall and the server's own program.
//!
//! The first time a JVM listens, Windows asks the person at the screen
//! whether to let it through. Dismiss that prompt (or nobody is there, on a
//! headless PC) and Windows writes *block* rules for that `java.exe` on public
//! networks. A block rule beats any "allow this port" rule, so the router
//! forwards fine and the server still cannot be reached — from the internet
//! and from the LAN. This finds those rules and, with the user's say-so,
//! replaces them with allow rules (which needs an administrator prompt).

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::AppResult;

/// What the firewall does with connections to one program.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallStatus {
    /// False off Windows, for containers, and when the rules can't be read.
    pub applies: bool,
    pub program: Option<String>,
    /// An enabled inbound block rule names this program.
    pub blocked: bool,
    /// An enabled inbound allow rule names this program.
    pub allowed: bool,
}

/// Read the rules for `program` (no elevation needed).
pub fn check(program: &Path) -> FirewallStatus {
    #[cfg(windows)]
    {
        windows::check(program)
    }
    #[cfg(not(windows))]
    {
        let _ = program;
        FirewallStatus::default()
    }
}

/// Remove the block rules for `program` and allow it in, behind a UAC prompt.
pub fn fix(program: &Path) -> AppResult<FirewallStatus> {
    #[cfg(windows)]
    {
        windows::fix(program)?;
        Ok(check(program))
    }
    #[cfg(not(windows))]
    {
        let _ = program;
        Ok(FirewallStatus::default())
    }
}

#[cfg(windows)]
mod windows {
    use std::path::Path;
    use std::process::Command;

    use super::FirewallStatus;
    use crate::error::{AppError, AppResult, ErrorCode};
    use crate::runtime::java::configure_no_window_command;

    /// Windows writes the rules with the path in lower case, so compare
    /// without case (`-ieq`). The path travels as an environment variable,
    /// never spliced into the script.
    const CHECK: &str = "$p=$env:NB_PROGRAM;\
        $r=@(Get-NetFirewallApplicationFilter | Where-Object { $_.Program -ieq $p } | Get-NetFirewallRule |\
        Where-Object { $_.Enabled -eq 'True' -and $_.Direction -eq 'Inbound' });\
        '{0} {1}' -f @($r | Where-Object Action -eq 'Block').Count, @($r | Where-Object Action -eq 'Allow').Count";

    /// Windows PowerShell by its full path: the app may run with a PATH that
    /// lacks it.
    fn powershell() -> std::path::PathBuf {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
        std::path::Path::new(&root).join(r"System32\WindowsPowerShell\v1.0\powershell.exe")
    }

    pub fn check(program: &Path) -> FirewallStatus {
        let output = configure_no_window_command(&mut Command::new(powershell()))
            .args(["-NoProfile", "-NonInteractive", "-Command", CHECK])
            .env("NB_PROGRAM", program)
            .output();
        let Ok(output) = output else { return FirewallStatus::default() };
        let text = String::from_utf8_lossy(&output.stdout);
        let mut counts = text.split_whitespace().filter_map(|n| n.parse::<u32>().ok());
        let (Some(blocked), Some(allowed)) = (counts.next(), counts.next()) else {
            return FirewallStatus::default();
        };
        FirewallStatus {
            applies: true,
            program: Some(program.display().to_string()),
            blocked: blocked > 0,
            allowed: allowed > 0,
        }
    }

    pub fn fix(program: &Path) -> AppResult<()> {
        // The elevated process starts with a fresh environment, so the path
        // goes in the script as a single-quoted literal (quotes doubled).
        let literal = program.display().to_string().replace('\'', "''");
        let script = format!(
            "$p = '{literal}'\r\n\
             Get-NetFirewallApplicationFilter | Where-Object {{ $_.Program -ieq $p }} | Get-NetFirewallRule |\
             Where-Object {{ $_.Direction -eq 'Inbound' -and ($_.Action -eq 'Block' -or $_.Group -eq 'Nebula Host') }} | Remove-NetFirewallRule\r\n\
             foreach ($proto in 'TCP', 'UDP') {{\r\n\
               New-NetFirewallRule -DisplayName \"Nebula Host - Java ($proto)\" -Group 'Nebula Host' -Description $p\
               -Direction Inbound -Action Allow -Program $p -Protocol $proto -Profile Any | Out-Null\r\n\
             }}\r\n"
        );
        let file = std::env::temp_dir().join(format!("nebula-firewall-{}.ps1", uuid::Uuid::new_v4()));
        std::fs::write(&file, script)?;
        // Start-Process throws when the UAC prompt is declined.
        let elevate = "Start-Process (Join-Path $PSHOME powershell.exe) -Verb RunAs -Wait -WindowStyle Hidden \
            -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File',('\"' + $env:NB_SCRIPT + '\"')";
        let status = configure_no_window_command(&mut Command::new(powershell()))
            .args(["-NoProfile", "-NonInteractive", "-Command", elevate])
            .env("NB_SCRIPT", &file)
            .status();
        let _ = std::fs::remove_file(&file);
        match status {
            Ok(status) if status.success() => Ok(()),
            Ok(_) => Err(AppError::new(ErrorCode::Cancelled).with("reason", "firewall_fix_declined")),
            Err(error) => Err(error.into()),
        }
    }
}
