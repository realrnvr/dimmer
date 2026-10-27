//! Migrated from the former Python `a11y-autostart.py` harness: toggling
//! "Start minimised" and "Auto run" must create and remove the XDG autostart
//! entry with the `--minimised` flag.

mod common;

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};

const AUTOSTART_FILE: &str = "io.github.dimmer.Linux.desktop";

#[tokio::test]
async fn autostart_entry_follows_the_switches() -> Result<()> {
    let _guard = common::SERIAL.lock().await;
    common::assert_a11y_bus_ready()?;

    let config_home = tempfile::Builder::new()
        .prefix("dimmer-autostart-")
        .tempdir()
        .context("create temporary XDG_CONFIG_HOME")?;
    let config_home_value = config_home.path().to_string_lossy().into_owned();

    let launch = common::launch_with(&[], &[("XDG_CONFIG_HOME", config_home_value)], false).await?;
    {
        let session = &launch.session;
        let options = session.locate("//Tab[@name='Options']");
        common::pointer_click(&options).await?;
        options.wait_for_selected().await?;

        // libadwaita exposes each page's widget tree under two AT-SPI parents
        // (the view stack page and the tab's panel), so every control appears
        // twice in a snapshot. Pin the first occurrence; both refer to the same
        // widget, but a single-target locator rejects the duplicate.
        let start_minimised = session.locate("//Switch[@name='Start minimised']").first();
        let auto_run = session.locate("//Switch[@name='Auto run']").first();
        start_minimised.wait_for_visible().await?;
        auto_run.wait_for_visible().await?;

        set_switch(&start_minimised, true).await?;
        set_switch(&auto_run, true).await?;

        let entry = config_home.path().join("autostart").join(AUTOSTART_FILE);
        wait_for_path(&entry, true).await?;
        let text = std::fs::read_to_string(&entry).context("read autostart entry")?;
        anyhow::ensure!(
            text.contains("--minimised"),
            "autostart entry is missing --minimised:\n{text}"
        );
        anyhow::ensure!(
            text.contains("io.github.dimmer.Linux"),
            "autostart entry does not reference Dimmer:\n{text}"
        );

        set_switch(&auto_run, false).await?;
        wait_for_path(&entry, false).await?;

        set_switch(&start_minimised, false).await?;
    }

    launch.kill().await
}

async fn set_switch(switch: &waydriver::Locator, want: bool) -> Result<()> {
    if switch.is_checked().await? != want {
        common::pointer_click(switch).await?;
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if switch.is_checked().await? == want {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(anyhow!("switch {} did not become {want}", switch.xpath()));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn wait_for_path(path: &Path, present: bool) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if path.exists() == present {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(anyhow!(
                "{} stayed {}",
                path.display(),
                if present { "absent" } else { "present" }
            ));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
