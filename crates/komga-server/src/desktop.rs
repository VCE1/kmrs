//! Desktop installation boundaries: only an installed app or per-user Windows
//! installation can hand a verified update to its platform helper.

#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::path::Path;
use std::path::PathBuf;

use crate::service::desktop_update::DesktopUpdate;

pub fn update_handle() -> DesktopUpdate {
    DesktopUpdate::new(tokio::runtime::Handle::current())
}

pub fn status_label(status: &crate::service::desktop_update::Status) -> String {
    use crate::service::desktop_update::Status;
    match status {
        Status::Idle => "Check for Updates".into(),
        Status::Checking => "Checking for Updates…".into(),
        Status::Available(version) => format!("Update to {version} and Restart"),
        Status::Downloading(version) => format!("Downloading {version}…"),
        Status::Installing(version) => format!("Installing {version}…"),
        Status::Failed(_) => "Retry Update".into(),
    }
}

pub fn tooltip(status: &crate::service::desktop_update::Status) -> String {
    match status {
        crate::service::desktop_update::Status::Failed(error) => format!("Update failed: {error}"),
        _ => "KMServer".into(),
    }
}

pub fn busy(status: &crate::service::desktop_update::Status) -> bool {
    use crate::service::desktop_update::Status;
    matches!(
        status,
        Status::Checking | Status::Downloading(_) | Status::Installing(_)
    )
}

#[cfg(target_os = "macos")]
pub fn app_bundle_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let app = exe.parent()?.parent()?.parent()?;
    (app.extension()? == "app").then(|| app.to_path_buf())
}

#[cfg(target_os = "macos")]
pub fn start_update(
    update: DesktopUpdate,
    app: PathBuf,
    config_dir: PathBuf,
    port: u16,
    shutdown: tokio::sync::watch::Sender<bool>,
) {
    if let Err(e) = install_macos(&update, &app, &config_dir, port, &shutdown) {
        update.set_failed(e.clone());
        tracing::error!("macOS update failed: {e}");
    }
}

#[cfg(target_os = "macos")]
fn install_macos(
    update: &DesktopUpdate,
    app: &Path,
    config_dir: &Path,
    port: u16,
    shutdown: &tokio::sync::watch::Sender<bool>,
) -> Result<(), String> {
    use crate::service::desktop_update::Status;

    if !matches!(update.status(), Status::Available(_)) {
        return Err("no checked update is ready".into());
    };
    let parent = app.parent().ok_or("app has no parent directory")?;
    let work = tempfile::Builder::new()
        .prefix(".kmserver-update-")
        .tempdir_in(parent)
        .map_err(|e| format!("create update directory: {e}"))?;
    let downloaded = update.download_verified(work.path())?;
    unpack_app(&downloaded.path, work.path())?;
    let staged = work.path().join("staged/KMServer.app");
    verify_app(&staged)?;
    let helper = work.path().join("update-helper.sh");
    std::fs::write(
        &helper,
        include_bytes!("../resources/macos/update-helper.sh"),
    )
    .map_err(|e| format!("write update helper: {e}"))?;
    let script = format!(
        "nohup /bin/sh {} {} {} {} {} {} </dev/null >/dev/null 2>&1 &",
        shell_arg(&helper),
        shell_arg(Path::new(&std::process::id().to_string())),
        shell_arg(app),
        shell_arg(work.path()),
        shell_arg(config_dir),
        port,
    );
    let status = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(script)
        .status()
        .map_err(|e| format!("start update helper: {e}"))?;
    if !status.success() {
        return Err(format!("update helper failed to start: {status}"));
    }
    update.installing();
    // The detached helper owns the staging files from this point.
    std::mem::forget(work);
    let _ = shutdown.send(true);
    Ok(())
}

#[cfg(target_os = "macos")]
fn verify_app(app: &Path) -> Result<(), String> {
    for (program, args) in [
        ("/usr/bin/codesign", vec!["--verify", "--deep", "--strict"]),
        ("/usr/bin/spctl", vec!["--assess", "--type", "execute"]),
    ] {
        let status = std::process::Command::new(program)
            .args(args)
            .arg(app)
            .status()
            .map_err(|e| format!("verify update: {e}"))?;
        if !status.success() {
            return Err(format!("verify update: {status}"));
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn unpack_app(archive: &Path, directory: &Path) -> Result<(), String> {
    let status = std::process::Command::new("/usr/bin/ditto")
        .arg("-x")
        .arg("-k")
        .arg(archive)
        .arg(directory.join("staged"))
        .status()
        .map_err(|e| format!("unpack update: {e}"))?;
    if !status.success() {
        return Err(format!("unpack update: {status}"));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn shell_arg(path: &Path) -> String {
    let text = path.to_string_lossy();
    format!("'{}'", text.replace('\'', "'\\''"))
}

#[cfg(target_os = "windows")]
pub fn installed_exe() -> Option<PathBuf> {
    crate::tray::installed_exe()
}

#[cfg(target_os = "windows")]
pub fn start_update(
    update: DesktopUpdate,
    _app: PathBuf,
    config_dir: PathBuf,
    shutdown: tokio::sync::watch::Sender<bool>,
) {
    if let Err(e) = install_windows(&update, &config_dir, &shutdown) {
        update.set_failed(e.clone());
        tracing::error!("Windows update failed: {e}");
    }
}

#[cfg(target_os = "windows")]
fn install_windows(
    update: &DesktopUpdate,
    config_dir: &Path,
    shutdown: &tokio::sync::watch::Sender<bool>,
) -> Result<(), String> {
    use crate::service::desktop_update::Status;

    if !matches!(update.status(), Status::Available(_)) {
        return Err("no checked update is ready".into());
    };
    let directory = std::env::temp_dir().join(format!("kmserver-update-{}", std::process::id()));
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let downloaded = update.download_verified(&directory)?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let install_dir = exe.parent().ok_or("installed executable has no parent")?;
    let result = std::process::Command::new(&downloaded.path)
        .arg("/S")
        .arg(format!("/UPDATEPID={}", std::process::id()))
        .arg(format!(r#"/INSTALLDIR="{}""#, install_dir.display()))
        .arg(format!(r#"/CONFIGDIR="{}""#, config_dir.display()))
        .spawn();
    match result {
        Ok(_) => {
            update.installing();
            let _ = shutdown.send(true);
            Ok(())
        }
        Err(e) => Err(format!("start installer: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_is_actionable_without_hiding_progress() {
        assert_eq!(
            status_label(&crate::service::desktop_update::Status::Idle),
            "Check for Updates"
        );
        assert!(
            status_label(&crate::service::desktop_update::Status::Available(
                "1.2.3".into()
            ))
            .contains("1.2.3")
        );
        assert!(busy(&crate::service::desktop_update::Status::Installing(
            "1.2.3".into()
        )));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn shell_arguments_preserve_apostrophes() {
        assert_eq!(shell_arg(Path::new("/tmp/KM's app")), "'/tmp/KM'\\''s app'");
    }
}
