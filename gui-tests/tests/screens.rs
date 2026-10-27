//! Screen dim controls: the vertical slider, the +/- step buttons and the
//! numeric entry must stay in sync, and typed values are clamped to 0–90.
//!
//! The step buttons are driven through their AT-SPI `Action` rather than
//! synthesized pointer events. GTK reports window-relative extents for
//! widgets inside a `GtkFlowBox` about 20 px away from where they are drawn,
//! so a pointer click at the reported centre lands outside these 34 px
//! buttons. `Action` is exactly what a screen reader invokes, and the header
//! suite already covers real pointer routing.

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use waydriver::{Locator, Session};

const ENTRY: &str = "//TextBox[@name='Screen 1 dim percentage (0–90)']";
const SLIDER: &str = "//Slider[@name='Screen 1 dim percentage (0–90)']";
const INCREASE: &str = "//Button[@name='Increase Screen 1 dim percentage (0–90)']";
const DECREASE: &str = "//Button[@name='Decrease Screen 1 dim percentage (0–90)']";
const FOCUSED: &str = "//*[@focused='true']";

#[tokio::test]
async fn screen_dim_controls_stay_in_sync() -> Result<()> {
    let _guard = common::SERIAL.lock().await;
    common::assert_a11y_bus_ready()?;
    let launch = common::launch().await?;

    {
        let session = &launch.session;
        let entry = session.locate(ENTRY).first();
        entry
            .with_timeout(Duration::from_secs(10))
            .wait_for_present()
            .await
            .context("the dim percentage entry never appeared")?;

        anyhow::ensure!(
            entry.text().await? == "30",
            "entry should start at 30, got {:?}",
            entry.text().await?
        );

        session.locate(INCREASE).first().click().await?;
        wait_for_text(&entry, "31").await?;

        session.locate(DECREASE).first().click().await?;
        wait_for_text(&entry, "30").await?;

        // Typing an exact value and confirming with Enter moves the slider too.
        focus_entry(session).await?;
        entry.fill_assume_focused("45", waydriver::FillMode::CaretNav).await?;
        session.press_chord("Return").await?;
        wait_for_text(&entry, "45").await?;
        let slider = session.locate(SLIDER).first();
        anyhow::ensure!(
            (slider.value().await?.current - 45.0).abs() < f64::EPSILON,
            "slider did not follow the typed value: {:?}",
            slider.value().await?.current
        );

        // Out-of-range input is clamped instead of rejected silently.
        entry.fill_assume_focused("500", waydriver::FillMode::CaretNav).await?;
        session.press_chord("Return").await?;
        wait_for_text(&entry, "90").await?;
        anyhow::ensure!(
            (slider.value().await?.current - 90.0).abs() < f64::EPSILON,
            "slider did not clamp to 90: {:?}",
            slider.value().await?.current
        );
    }

    launch.kill().await
}

/// Tabs forward until the entry owns keyboard focus. `GtkEntry` has no AT-SPI
/// `Component::grab_focus`, and a pointer click cannot be used here (see the
/// module docs), so keyboard traversal is the reliable way in.
async fn focus_entry(session: &Arc<Session>) -> Result<()> {
    for _ in 0..20 {
        let focused = session.locate(FOCUSED).inspect_all().await?;
        if focused.iter().any(|element| {
            element.role == "TextBox"
                && element.name.as_deref() == Some("Screen 1 dim percentage (0–90)")
        }) {
            return Ok(());
        }
        session.press_chord("Tab").await?;
        tokio::time::sleep(Duration::from_millis(80)).await;
    }
    let focused = session.locate(FOCUSED).inspect_all().await?;
    Err(anyhow!(
        "the dim percentage entry never received focus; last focus: {:?}",
        focused
            .iter()
            .map(|element| (element.role.as_str(), element.name.as_deref()))
            .collect::<Vec<_>>()
    ))
}

async fn wait_for_text(entry: &Locator, want: &str) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let text = entry.text().await?;
        if text == want {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(anyhow!("entry stayed {text:?}, expected {want:?}"));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
