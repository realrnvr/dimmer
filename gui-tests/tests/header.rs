//! Regression tests for the 2026-10 header-click bug.
//!
//! The whole `adw::HeaderBar` used to own a `GestureDrag` for window moving.
//! That parent gesture won the press sequence before `adw::ViewSwitcher` tabs
//! and the end-cap buttons could activate, so only content controls (slider,
//! colour picker, banner button) responded to the mouse.
//!
//! These tests drive the real Wayland pointer through WayDriver; they never
//! call AT-SPI `Action.DoAction`, so a regression that reintroduces the
//! gesture would fail here.
//!
//! The donation test lives in its own binary (`tests/donate.rs`) because the
//! mock portal sink must own `org.freedesktop.portal.Desktop` before the app
//! starts. A shared suite would let the real portal be D-Bus-activated first.

mod common;

use std::time::Duration;

use anyhow::{Context, Result};

#[tokio::test]
async fn every_header_tab_switches_with_the_pointer() -> Result<()> {
    let _guard = common::SERIAL.lock().await;
    common::assert_a11y_bus_ready()?;
    let launch = common::launch().await?;

    {
        let session = &launch.session;
        let screens = session.locate("//Tab[@name='Screens']");
        screens.wait_for_selected().await?;

        for page in ["Options", "About", "Debug", "Screens"] {
            let tab = session.locate(&format!("//Tab[@name='{page}']"));
            tab.wait_for_visible().await?;
            common::pointer_click(&tab).await?;
            tab.wait_for_selected()
                .await
                .with_context(|| format!("clicking the {page} tab did not select it"))?;
        }
    }

    launch.kill().await
}

#[tokio::test]
async fn minimise_button_hides_and_show_window_restores() -> Result<()> {
    let _guard = common::SERIAL.lock().await;
    common::assert_a11y_bus_ready()?;
    // Headless Mutter ignores `gtk_window_minimize()`, so use the branch the
    // app takes when "Show in taskbar" is off: `set_visible(false)`. That is
    // observable through AT-SPI and exercises the same button handler.
    let launch = common::launch_with_config(r#"{"ShowInTaskbar": false}"#).await?;

    {
        let session = &launch.session;
        let minimise = session.locate("//Button[@name='Minimise']");
        minimise.wait_for_visible().await?;
        common::pointer_click(&minimise).await?;
        common::wait_until_hidden(session, "//Window[@name='Dimmer']", Duration::from_secs(5))
            .await?;

        common::show_window()?;
        common::wait_until_shown(session, "//Window[@name='Dimmer']", Duration::from_secs(5))
            .await?;

        // The header must still be usable after the round trip.
        let options = session.locate("//Tab[@name='Options']");
        common::pointer_click(&options).await?;
        options.wait_for_selected().await?;
    }

    launch.kill().await
}

#[tokio::test]
async fn close_button_quits_the_application() -> Result<()> {
    let _guard = common::SERIAL.lock().await;
    common::assert_a11y_bus_ready()?;
    let launch = common::launch().await?;

    {
        let session = &launch.session;
        let close = session.locate("//Button[@name='Close']");
        close.wait_for_visible().await?;
        common::pointer_click(&close).await?;

        common::wait_until_gone(session, "//Window[@name='Dimmer']", Duration::from_secs(10))
            .await?;
    }

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while common::app_on_bus() {
        if std::time::Instant::now() >= deadline {
            anyhow::bail!("Dimmer is still on the bus after Close");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    launch.kill().await
}
