#![allow(dead_code)]

//! Shared WayDriver harness for Dimmer's GUI tests.
//!
//! Each test gets its own headless Mutter session, a private D-Bus bus (the
//! caller runs the suite inside `dbus-run-session`) and an isolated config
//! file, so tests never touch the developer's desktop or real `Dimmer.ini`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use tempfile::TempDir;
use tokio::sync::Mutex;
use waydriver::{
    CompositorRuntime, GSettingEntry, GSettingsConfig, Locator, Session, SessionConfig,
};
use waydriver_capture_mutter::MutterCapture;
use waydriver_compositor_mutter::MutterCompositor;
use waydriver_input_mutter::MutterInput;

/// WayDriver's `Locator` owns a `Session` handle; all locators in one test
/// share one compositor, so run tests one at a time.
pub static SERIAL: Mutex<()> = Mutex::const_new(());

/// D-Bus identity advertised by the application.
const APP_BUS: &str = "io.github.dimmer.Linux";
const APP_PATH: &str = "/io/github/dimmer/Linux";

/// A running Dimmer instance plus its isolated config directory.
pub struct Launch {
    pub session: Arc<Session>,
    config_dir: TempDir,
}

impl Launch {
    /// Stops the app and compositor.
    ///
    /// `Session::kill` consumes the session, but every [`Locator`] holds its
    /// own `Arc<Session>` handle. Tests therefore scope their locators so the
    /// `Launch` is the last owner by the time they shut down. If a locator is
    /// still alive, fall back to cancellation: the session's final drop runs
    /// WayDriver's best-effort teardown.
    pub async fn kill(self) -> Result<()> {
        self.session.cancel();
        match Arc::try_unwrap(self.session) {
            Ok(session) => session.kill().await.map_err(Into::into),
            Err(shared) => {
                shared.cancel();
                Ok(())
            }
        }
    }
}

/// Starts Dimmer with `--show` and an isolated config file.
pub async fn launch() -> Result<Launch> {
    launch_with(&[], &[], false).await
}

/// Starts Dimmer with an initial `Dimmer.ini` document, e.g.
/// `{"ShowInTaskbar": false}`.
pub async fn launch_with_config(settings: &str) -> Result<Launch> {
    launch_full(Some(settings), &[], &[], false).await
}

/// Starts Dimmer with extra CLI arguments and environment variables.
pub async fn launch_with(
    extra_args: &[&str],
    extra_env: &[(&str, String)],
    capture_external_effects: bool,
) -> Result<Launch> {
    launch_full(None, extra_args, extra_env, capture_external_effects).await
}

async fn launch_full(
    initial_settings: Option<&str>,
    extra_args: &[&str],
    extra_env: &[(&str, String)],
    capture_external_effects: bool,
) -> Result<Launch> {
    let binary = dimmer_binary();
    let config_dir = tempfile::Builder::new()
        .prefix("dimmer-gui-")
        .tempdir()
        .context("create temporary config directory")?;
    let config_file = config_dir.path().join("Dimmer.ini");
    if let Some(settings) = initial_settings {
        std::fs::write(&config_file, settings).context("write initial Dimmer.ini")?;
    }

    let mut compositor = MutterCompositor::new().with_gsettings(GSettingsConfig {
        isolated: true,
        initial: vec![GSettingEntry::new(
            "org.gnome.desktop.interface",
            "toolkit-accessibility",
            "true",
        )],
    });
    compositor.start(None, None).await?;
    let state = compositor.state().ok_or_else(|| anyhow!("Mutter state missing after start"))?;
    let input = MutterInput::new(state.clone());
    let capture = MutterCapture::new(state);

    let mut args = vec![
        "--config".to_string(),
        config_file.to_string_lossy().into_owned(),
        "--show".to_string(),
    ];
    args.extend(extra_args.iter().map(ToString::to_string));

    let session = Session::start(
        Box::new(compositor),
        Box::new(input),
        Box::new(capture),
        SessionConfig {
            command: binary.to_string_lossy().into_owned(),
            args,
            cwd: None,
            app_name: "Dimmer".into(),
            video_output: None,
            video_bitrate: None,
            video_fps: None,
            prewarm_visual: false,
            visual_region_tuning: Default::default(),
            visual_text_tuning: Default::default(),
            visual_click_tuning: Default::default(),
            gsettings_isolated: true,
            xdg_isolated: true,
            extra_env: extra_env
                .iter()
                .map(|(key, value)| ((*key).to_string(), value.clone()))
                .collect(),
            capture_external_effects,
        },
    )
    .await?;

    let launch = Launch { session: Arc::new(session), config_dir };
    wait_for_app(&launch.session).await?;
    Ok(launch)
}

/// Path to the built `dimmer` binary.
///
/// `DIMMER_BIN` wins; otherwise the release build next to this crate is used,
/// falling back to the debug build.
fn dimmer_binary() -> PathBuf {
    if let Some(path) = std::env::var_os("DIMMER_BIN") {
        return PathBuf::from(path);
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("gui-tests lives at <repo>/dimmer/gui-tests");
    let release = root.join("target/release/dimmer");
    if release.is_file() { release } else { root.join("target/debug/dimmer") }
}

/// Waits until the four page tabs are laid out.
pub async fn wait_for_app(session: &Arc<Session>) -> Result<()> {
    session
        .locate("//Tab[@name='Screens']")
        .with_timeout(Duration::from_secs(15))
        .wait_for_visible()
        .await
        .context("Dimmer window did not expose its page tabs")?;
    Ok(())
}

/// Clicks an element with real synthesized pointer events (not AT-SPI
/// `Action.DoAction`) so gesture handling is exercised end to end.
pub async fn pointer_click(locator: &Locator) -> Result<()> {
    locator
        .pointer_click(waydriver::PointerButton::Left)
        .await
        .with_context(|| format!("pointer click failed for {}", locator.xpath()))?;
    Ok(())
}

/// Polls `count()` until the selector disappears or the timeout expires.
pub async fn wait_until_gone(session: &Arc<Session>, xpath: &str, timeout: Duration) -> Result<()> {
    let locator = session.locate(xpath);
    let deadline = Instant::now() + timeout;
    loop {
        if locator.count().await.unwrap_or(0) == 0 {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(anyhow!("{xpath} still present after {timeout:?}"));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Polls until the selector is gone *or* no longer showing (minimised windows
/// may stay in the AT-SPI tree with `showing=false`).
pub async fn wait_until_hidden(
    session: &Arc<Session>,
    xpath: &str,
    timeout: Duration,
) -> Result<()> {
    let locator = session.locate(xpath);
    let deadline = Instant::now() + timeout;
    loop {
        if locator.count().await.unwrap_or(0) == 0 {
            return Ok(());
        }
        if !locator.is_showing().await.unwrap_or(false) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(anyhow!("{xpath} still showing after {timeout:?}"));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Polls until the selector is at least allocatable and showing.
pub async fn wait_until_shown(
    session: &Arc<Session>,
    xpath: &str,
    timeout: Duration,
) -> Result<()> {
    let locator = session.locate(xpath);
    let deadline = Instant::now() + timeout;
    loop {
        if locator.count().await.unwrap_or(0) > 0 && locator.is_showing().await.unwrap_or(false) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(anyhow!("{xpath} still hidden after {timeout:?}"));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Calls a method on Dimmer's D-Bus interface (used to restore a minimised
/// window from the tray).
pub fn call_app(method: &str) -> Result<()> {
    let output = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            APP_BUS,
            "--object-path",
            APP_PATH,
            "--method",
            &format!("{APP_BUS}.{method}"),
        ])
        .output()
        .context("run gdbus")?;
    if !output.status.success() {
        return Err(anyhow!(
            "gdbus {method} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

/// Asks the running app to bring its window back.
pub fn show_window() -> Result<()> {
    call_app("ShowWindow")
}

/// Whether the app still owns its well-known D-Bus name.
pub fn app_on_bus() -> bool {
    Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.freedesktop.DBus",
            "--object-path",
            "/org/freedesktop/DBus",
            "--method",
            "org.freedesktop.DBus.NameHasOwner",
            APP_BUS,
        ])
        .output()
        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).contains("true"))
}

/// Re-executes the suite's accessibility bootstrap once per process.
///
/// Fedora's SELinux policy can refuse D-Bus activation of
/// `org.a11y.Bus`/`org.a11y.atspi.Registry` inside a nested
/// `dbus-run-session`. The wrapper script starts both daemons explicitly;
/// this guard simply verifies they are reachable so a misconfigured run fails
/// with a clear message instead of a 15-second app-discovery timeout.
pub fn assert_a11y_bus_ready() -> Result<()> {
    static READY: OnceLock<Result<(), String>> = OnceLock::new();
    let result = READY.get_or_init(|| {
        let output = Command::new("gdbus")
            .args([
                "call",
                "--session",
                "--dest",
                "org.a11y.Bus",
                "--object-path",
                "/org/a11y/bus",
                "--method",
                "org.a11y.Bus.GetAddress",
            ])
            .output()
            .map_err(|error| format!("run gdbus: {error}"))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
        }
    });
    match result {
        Ok(()) => Ok(()),
        Err(error) => Err(anyhow!(
            "accessibility bus unavailable ({error}); run gui-tests/run.sh instead of cargo test directly"
        )),
    }
}
