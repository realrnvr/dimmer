//! Donation button test, isolated in its own binary.
//!
//! WayDriver's mock portal sink has to own
//! `org.freedesktop.portal.Desktop` *before* Dimmer starts. Running this in a
//! fresh `dbus-run-session` (see `run.sh`) keeps a previously activated real
//! `xdg-desktop-portal` from winning the name on a shared test bus.

mod common;

use std::time::Duration;

use anyhow::{Context, Result};

#[tokio::test]
async fn donate_button_opens_paypal() -> Result<()> {
    let _guard = common::SERIAL.lock().await;
    common::assert_a11y_bus_ready()?;
    let launch = common::launch_with(&[], &[], true).await?;

    {
        let session = &launch.session;
        let cursor = session.open_uri_cursor()?;
        let donate = session.locate("//Button[@name='Donate with PayPal']");
        donate.wait_for_visible().await?;
        common::pointer_click(&donate).await?;

        let uri = session
            .wait_for_open_uri(
                cursor,
                |request| request.uri.contains("paypal.com"),
                Duration::from_secs(5),
            )
            .await
            .context("Donate button did not request the PayPal URL")?;
        assert!(uri.uri.contains("N9EWT4F7JBWFE"), "unexpected donation URL: {}", uri.uri);
    }

    launch.kill().await
}
