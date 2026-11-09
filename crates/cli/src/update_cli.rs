//! `zeren update` — check for and apply a newer release, natively (the same
//! flow `edge/src/install.sh` performs: download → verify → symlink swap →
//! service restart). macOS app bundles swap the bundle instead; source builds
//! are report-only.

use anyhow::bail;
use zeren_update::{InstallKind, Offer};

/// `--check` prints the verdict and exits (nonzero when an update is available,
/// so scripts can gate on it). A macOS app follows its release channel
/// (`update-channel.json` in the data dir), as the app's own updater does.
pub async fn update(edge_url: &str, check_only: bool) -> anyhow::Result<()> {
    zeren_update::channel::set_data_dir(&super::paths::data_dir());
    let manifest = zeren_update::fetch_latest(edge_url).await?;
    let current = zeren_update::app_version();
    let running_engine = match zeren_update::detect_install() {
        InstallKind::MacApp { bundle } => zeren_update::bundle_engine_version(&bundle),
        _ => None,
    };
    match zeren_update::offer(
        &manifest,
        current,
        manifest.channel,
        running_engine.as_deref(),
        zeren_update::channel::macos_version(),
    ) {
        Offer::Newer | Offer::Switch => {}
        Offer::Held(why) => {
            println!(
                "zeren {current}: {} isn't installable here. {why}",
                manifest.version
            );
            return Ok(());
        }
        Offer::Current => {
            println!(
                "zeren {current} is up to date (latest: {}).",
                manifest.version
            );
            return Ok(());
        }
    }
    println!("zeren {current} → {} available", manifest.version);
    if check_only {
        std::process::exit(1);
    }

    let install = zeren_update::detect_install();
    if let Some(blocker) = install.desktop_update_blocker() {
        bail!("{blocker}");
    }
    match install {
        InstallKind::Managed { app_root } if service_running() => {
            println!(
                "downloading {}…",
                zeren_update::headless_artifact(&manifest.version)
            );
            let data_dir = super::paths::data_dir();
            zeren_update::prepare_install(edge_url, &manifest, &app_root, &data_dir).await?;
            let from = zeren_update::installed_version(&InstallKind::Managed {
                app_root: app_root.clone(),
            })
            .unwrap_or_else(|| zeren_update::current_version().to_owned());
            let requested = zeren_update::transaction::request(
                &data_dir,
                zeren_update::transaction::Layout::Versioned {
                    app_root: app_root.clone(),
                },
                &from,
                &manifest.version,
                zeren_update::transaction::RestartMode::Service {
                    unit: zeren_update::service_unit(),
                },
            )?;
            if let Err(err) = zeren_update::restart_service() {
                zeren_update::transaction::fail_request(
                    &data_dir,
                    &requested.id,
                    "Zeren couldn't restart to install the update, so nothing changed.".into(),
                );
                bail!("couldn't restart the engine service: {err:#}");
            }
            println!(
                "engine service restarting; waiting for {} to prove it's ready…",
                manifest.version
            );
            wait_for_outcome(&data_dir, &requested).await
        }
        InstallKind::Managed { app_root } => {
            println!(
                "downloading {}…",
                zeren_update::headless_artifact(&manifest.version)
            );
            zeren_update::stage_headless(edge_url, &manifest, &app_root).await?;
            zeren_update::apply_headless(&app_root, &manifest.version)?;
            println!(
                "installed {} (current → {})",
                app_root.join(&manifest.version).display(),
                manifest.version
            );
            match zeren_update::restart_service() {
                Ok(()) => println!("engine service restarted."),
                Err(err) => println!(
                    "note: service restart failed ({err:#}) — restart the engine manually to finish."
                ),
            }
            Ok(())
        }
        InstallKind::MacApp { bundle } => {
            println!(
                "downloading {}…",
                zeren_update::mac_app_artifact(&manifest.version)
            );
            let data_dir = super::paths::data_dir();
            let staged = zeren_update::stage_mac_app(edge_url, &manifest, &data_dir).await?;
            zeren_update::apply_mac_app(&staged, &bundle)?;
            println!("updated {} — relaunch Zeren to finish.", bundle.display());
            Ok(())
        }
        #[cfg(windows)]
        InstallKind::WindowsPortable { directory } => {
            let staged = zeren_update::windows::stage(edge_url, &manifest, &directory).await?;
            zeren_update::windows::apply(&staged, &directory, false)?;
            println!(
                "updated to {} — relaunch Zeren to finish.",
                manifest.version
            );
            Ok(())
        }
        InstallKind::Unmanaged => {
            bail!(
                "this binary is not update-managed (source build or hand-copied).\n\
                 Linux: curl -fsSL https://zeron.sh/install.sh | sh, or run install.sh from the release tarball\n\
                 macOS: download the new Zeren.app dmg, or rebuild from source.\n\
                 Windows: install with the Zeren setup .exe from {}, or rebuild from source.",
                zeren_update::LATEST_RELEASE_PAGE
            )
        }
    }
}

/// Whether the installed service is up: it's what restarts into the update and
/// what can go back. Without one there's nothing running to protect, and the new
/// version simply starts next time.
fn service_running() -> bool {
    cfg!(target_os = "linux")
        && std::process::Command::new("systemctl")
            .args([
                "--user",
                "--quiet",
                "is-active",
                &zeren_update::service_unit(),
            ])
            .status()
            .is_ok_and(|status| status.success())
}

/// Report how the update went: it may yet go back, so "restarted" says nothing.
async fn wait_for_outcome(
    data_dir: &std::path::Path,
    requested: &zeren_update::transaction::Transaction,
) -> anyhow::Result<()> {
    use zeren_update::UpdateState;
    // The trial's own deadline, plus time to stop and go back.
    let limit = std::time::Duration::from_secs(requested.deadline_secs + 120);
    let started = std::time::Instant::now();
    while started.elapsed() < limit {
        let report = zeren_update::transaction::report(data_dir).filter(|r| r.id == requested.id);
        match report.as_ref().map(|report| report.state) {
            Some(UpdateState::Updated) => {
                println!("updated to {}.", requested.target);
                return Ok(());
            }
            Some(UpdateState::RolledBack | UpdateState::Failed) => {
                let reason = report.and_then(|report| report.reason).unwrap_or_default();
                bail!("the update didn't go through. {reason}");
            }
            _ => tokio::time::sleep(std::time::Duration::from_millis(500)).await,
        }
    }
    bail!(
        "the update hasn't finished after {} min. Check `zeren daemon status`.",
        limit.as_secs() / 60
    )
}
