//! Install/uninstall system-level autostart for the uninorm daemon.
//!
//! - macOS: LaunchAgent plist in ~/Library/LaunchAgents/
//! - Linux: systemd user unit in ~/.config/systemd/user/

use std::path::PathBuf;

use crate::error::DaemonError;

#[cfg(target_os = "macos")]
const MACOS_LABEL: &str = "com.uninorm.daemon";
#[cfg(target_os = "linux")]
const LINUX_UNIT: &str = "uninorm.service";

/// Install autostart so the daemon launches on login/boot.
pub fn install() -> Result<(), DaemonError> {
    let exe = std::env::current_exe().map_err(DaemonError::Spawn)?;

    #[cfg(target_os = "macos")]
    {
        install_launchagent(&exe)
    }

    #[cfg(target_os = "linux")]
    {
        install_systemd_unit(&exe)
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = exe;
        Err(DaemonError::UnsupportedPlatform)
    }
}

/// Uninstall autostart.
pub fn uninstall() -> Result<(), DaemonError> {
    #[cfg(target_os = "macos")]
    {
        uninstall_launchagent()
    }

    #[cfg(target_os = "linux")]
    {
        uninstall_systemd_unit()
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Err(DaemonError::UnsupportedPlatform)
    }
}

/// Check if autostart is currently installed.
pub fn is_installed() -> bool {
    autostart_path().is_some_and(|p| p.exists())
}

/// Return the path where the autostart config lives (if determinable).
pub fn autostart_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        dirs::home_dir().map(|h| {
            h.join("Library")
                .join("LaunchAgents")
                .join(format!("{MACOS_LABEL}.plist"))
        })
    }

    #[cfg(target_os = "linux")]
    {
        dirs::config_dir().map(|c| c.join("systemd").join("user").join(LINUX_UNIT))
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

/// XML-escape a string for safe embedding in plist XML values.
#[cfg(target_os = "macos")]
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

// -- macOS LaunchAgent --

#[cfg(target_os = "macos")]
fn install_launchagent(exe: &std::path::Path) -> Result<(), DaemonError> {
    let plist_path = autostart_path().ok_or(DaemonError::Io(std::io::Error::other(
        "could not determine LaunchAgents directory",
    )))?;

    if let Some(parent) = plist_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            DaemonError::Io(std::io::Error::other(format!(
                "failed to create LaunchAgents dir: {e}"
            )))
        })?;
    }

    let log_path = crate::config::log_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "/tmp/uninorm.log".to_string());

    let exe_escaped = xml_escape(&exe.display().to_string());
    let log_escaped = xml_escape(&log_path);

    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{MACOS_LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe}</string>
        <string>daemon-run</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <false/>
    <key>StandardOutPath</key>
    <string>{log}</string>
    <key>StandardErrorPath</key>
    <string>{log}</string>
</dict>
</plist>
"#,
        exe = exe_escaped,
        log = log_escaped,
    );

    std::fs::write(&plist_path, plist).map_err(|e| {
        DaemonError::Io(std::io::Error::other(format!("failed to write plist: {e}")))
    })?;

    // Load immediately so the daemon starts right away (not just on next login)
    let _ = std::process::Command::new("launchctl")
        .args(["load", &plist_path.display().to_string()])
        .output();

    Ok(())
}

#[cfg(target_os = "macos")]
fn uninstall_launchagent() -> Result<(), DaemonError> {
    let plist_path = autostart_path().ok_or(DaemonError::Io(std::io::Error::other(
        "could not determine LaunchAgents directory",
    )))?;

    if !plist_path.exists() {
        return Ok(());
    }

    let _ = std::process::Command::new("launchctl")
        .args(["unload", &plist_path.display().to_string()])
        .output();

    std::fs::remove_file(&plist_path).map_err(|e| {
        DaemonError::Io(std::io::Error::other(format!(
            "failed to remove plist: {e}"
        )))
    })?;

    Ok(())
}

// -- Linux systemd user unit --

#[cfg(target_os = "linux")]
fn install_systemd_unit(exe: &std::path::Path) -> Result<(), DaemonError> {
    let unit_path = autostart_path().ok_or(DaemonError::Io(std::io::Error::other(
        "could not determine systemd user unit directory",
    )))?;

    if let Some(parent) = unit_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            DaemonError::Io(std::io::Error::other(format!(
                "failed to create systemd user dir: {e}"
            )))
        })?;
    }

    // Escape spaces per systemd.service(5) spec
    let exe_str = exe.display().to_string().replace(' ', "\\x20");
    let unit = format!(
        r#"[Unit]
Description=uninorm NFD→NFC file watcher daemon
After=default.target

[Service]
Type=simple
ExecStart={exe} daemon-run
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
"#,
        exe = exe_str,
    );

    std::fs::write(&unit_path, unit).map_err(|e| {
        DaemonError::Io(std::io::Error::other(format!(
            "failed to write systemd unit: {e}"
        )))
    })?;

    // daemon-reload + enable
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .output();

    let output = std::process::Command::new("systemctl")
        .args(["--user", "enable", LINUX_UNIT])
        .output()
        .map_err(|e| {
            DaemonError::Io(std::io::Error::other(format!(
                "failed to run systemctl enable: {e}"
            )))
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(DaemonError::Io(std::io::Error::other(format!(
            "systemctl enable failed: {stderr}"
        ))));
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn uninstall_systemd_unit() -> Result<(), DaemonError> {
    let unit_path = autostart_path().ok_or(DaemonError::Io(std::io::Error::other(
        "could not determine systemd user unit directory",
    )))?;

    if !unit_path.exists() {
        return Ok(());
    }

    let _ = std::process::Command::new("systemctl")
        .args(["--user", "disable", "--now", LINUX_UNIT])
        .output();

    let _ = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .output();

    std::fs::remove_file(&unit_path).map_err(|e| {
        DaemonError::Io(std::io::Error::other(format!(
            "failed to remove systemd unit: {e}"
        )))
    })?;

    Ok(())
}
