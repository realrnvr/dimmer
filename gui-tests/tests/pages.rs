//! Migrated from the former Python `a11y-pages.py` harness: page traversal,
//! keyboard focus and the extension banner dialog, driven through WayDriver.

mod common;

use std::time::Duration;

use anyhow::{Context, Result};

const FOCUSABLE: &str = "//*[@focusable='true' and @showing='true']";
const FOCUSED: &str = "//*[@focused='true']";

/// Roles whose accessible name is legitimately empty while still focusable.
const NAMELESS_OK: &[&str] =
    &["Panel", "Group", "ScrollPane", "TabList", "GridCell", "Image", "Generic"];

/// Each page plus a focusable control that only exists once the page has been
/// realized. GTK switches tabs asynchronously, so waiting for the sentinel
/// avoids enumerating a page that has not been laid out (and therefore has no
/// accessibility children) yet.
const PAGES: &[(&str, &str)] = &[
    ("Screens", "//Button[@name='Increase Screen 1 dim percentage (0–90)' and @focusable='true']"),
    ("Options", "//Switch[@name='Show screen identity' and @focusable='true']"),
    ("About", "//Link[@name='Original Dimmer' and @focusable='true']"),
    ("Debug", "//Button[@name='Refresh' and @focusable='true']"),
];

#[tokio::test]
async fn every_page_has_named_focusable_controls() -> Result<()> {
    let _guard = common::SERIAL.lock().await;
    common::assert_a11y_bus_ready()?;
    let launch = common::launch().await?;

    {
        let session = &launch.session;
        for (page, sentinel) in PAGES {
            let tab = session.locate(&format!("//Tab[@name='{page}']"));
            common::pointer_click(&tab).await?;
            tab.wait_for_selected()
                .await
                .with_context(|| format!("{page} tab did not activate"))?;
            session
                .locate(sentinel)
                .with_timeout(Duration::from_secs(5))
                .wait_for_present()
                .await
                .with_context(|| format!("{page} page did not expose {sentinel}"))?;

            let controls = session.locate(FOCUSABLE).inspect_all().await?;
            let named = controls
                .iter()
                .filter(|element| {
                    element.name.as_deref().is_some_and(|name| !name.trim().is_empty())
                })
                .count();
            anyhow::ensure!(
                named > 0,
                "{page} exposes no named, focusable controls ({} focusable total)",
                controls.len()
            );

            // Tab through the page and make sure focus stays in the window and on
            // something the screen reader can announce.
            for step in 0..12 {
                session.press_chord("Tab").await?;
                tokio::time::sleep(Duration::from_millis(250)).await;
                let focused = session.locate(FOCUSED).inspect_all().await?;
                anyhow::ensure!(
                    !focused.is_empty(),
                    "{page}: focus was lost after {} Tab presses",
                    step + 1
                );
                for element in &focused {
                    let named = element.name.as_deref().is_some_and(|name| !name.trim().is_empty());
                    anyhow::ensure!(
                        named || NAMELESS_OK.contains(&element.role.as_str()),
                        "{page}: focused {} has no accessible name",
                        element.role
                    );
                }
            }

            println!("{page}: {} named focusable controls", named);
        }
    }

    launch.kill().await
}

#[tokio::test]
async fn banner_details_dialog_opens_and_closes_with_escape() -> Result<()> {
    let _guard = common::SERIAL.lock().await;
    common::assert_a11y_bus_ready()?;
    let launch = common::launch().await?;

    {
        let session = &launch.session;
        let details = session.locate("//Button[@name='Details']");
        // The banner appears ~1.5 s after startup when the extension is not
        // connected; that is the only state the headless harness can create.
        details.with_timeout(Duration::from_secs(5)).wait_for_visible().await?;
        common::pointer_click(&details).await?;

        let dialog = session.locate("//Dialog");
        dialog
            .with_timeout(Duration::from_secs(5))
            .wait_for_visible()
            .await
            .context("Details dialog did not open")?;

        session.press_chord("Escape").await?;
        common::wait_until_gone(session, "//Dialog", Duration::from_secs(5)).await?;
    }

    launch.kill().await
}
