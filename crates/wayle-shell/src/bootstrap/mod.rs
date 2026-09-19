//! Application bootstrap: service initialization and instance detection.

mod wallpaper;
mod weather;

use std::{
    error::Error,
    fmt::Display,
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::task::JoinHandle;
use tracing::{debug, info, warn};
use wayle_audio::AudioService;
use wayle_battery::BatteryService;
use wayle_bluetooth::BluetoothService;
use wayle_brightness::BrightnessService;
use wayle_config::{ConfigService, infrastructure::schema};
use wayle_core::{DeferredService, Property};
use wayle_hyprland::HyprlandService;
use wayle_ipc::shell::APP_ID;
use wayle_mango::MangoService;
use wayle_media::{MediaService, core::player::Player, types::PlaybackState};
use wayle_network::NetworkService;
use wayle_niri::NiriService;
use wayle_notification::NotificationService;
use wayle_power_profiles::PowerProfilesService;
use wayle_sysinfo::SysinfoService;
use wayle_systray::{SystemTrayService, types::TrayMode};
use wayle_wallpaper::WallpaperService;
use zbus::{Connection, fdo::DBusProxy};

use crate::{
    services::{IdleInhibitService, ShellIpcService},
    shell::ShellServices,
    startup::StartupTimer,
    watchers::build_extractor_config,
};

async fn spawned<T, E: Display>(handle: JoinHandle<Result<T, E>>) -> Result<T, String> {
    match handle.await {
        Ok(Ok(val)) => Ok(val),
        Ok(Err(err)) => Err(err.to_string()),
        Err(join_err) => Err(join_err.to_string()),
    }
}

macro_rules! try_service {
    ($timer:expr, $name:literal, $future:expr) => {
        match $timer.time($name, $future).await {
            Ok(service) => Some(Arc::new(service)),
            Err(e) => {
                warn!(error = %e, concat!($name, " unavailable"));
                None
            }
        }
    };
    ($timer:expr, $name:literal, $future:expr, no_wrap) => {
        match $timer.time($name, $future).await {
            Ok(service) => Some(service),
            Err(e) => {
                warn!(error = %e, concat!($name, " unavailable"));
                None
            }
        }
    };
}

struct CoreServices {
    battery: Option<Arc<BatteryService>>,
    brightness: Option<Arc<BrightnessService>>,
    idle_inhibit: Arc<IdleInhibitService>,
    network: Option<Arc<NetworkService>>,
    sysinfo: Arc<SysinfoService>,
    wallpaper: Option<Arc<WallpaperService>>,
}

struct DaemonServices {
    audio: Option<Arc<AudioService>>,
    media: Option<Arc<MediaService>>,
    notification: Option<Arc<NotificationService>>,
    systray: Option<Arc<SystemTrayService>>,
}

struct OptionalServices {
    hyprland: Option<Arc<HyprlandService>>,
    mango: Option<Arc<MangoService>>,
    niri: Option<Arc<NiriService>>,
}

pub async fn is_already_running() -> bool {
    let start = Instant::now();

    let Ok(connection) = Connection::session().await else {
        return false;
    };

    let Ok(dbus) = DBusProxy::new(&connection).await else {
        return false;
    };

    let Ok(name) = APP_ID.try_into() else {
        return false;
    };

    let result = dbus.name_has_owner(name).await.unwrap_or(false);
    debug!(
        duration_ms = start.elapsed().as_millis() as u64,
        "DBus instance check"
    );
    result
}

pub async fn init_services() -> Result<(StartupTimer, ShellServices), Box<dyn Error>> {
    let mut timer = StartupTimer::new();

    if let Err(e) = timer
        .time("Schema", async { schema::ensure_schema_current() })
        .await
    {
        warn!(error = %e, "Could not write schema file");
    }

    let config_service = timer.time("Config", ConfigService::load()).await?;

    let bluetooth: DeferredService<BluetoothService> = DeferredService::new(None);
    let power_profiles: DeferredService<PowerProfilesService> = DeferredService::new(None);

    let (weather, core, daemons, optional) = {
        let config = config_service.config();
        let weather = timer.time_sync("Weather", || {
            weather::build_weather_service(&config.modules)
        });

        let (core, daemons, optional) = tokio::join!(
            init_core_services(&timer, config),
            init_daemon_services(&timer, &config.modules),
            init_optional_services(&timer),
        );

        (weather, core?, daemons, optional)
    };

    spawn_deferred_bluetooth(bluetooth.clone());
    spawn_deferred_power_profiles(power_profiles.clone());

    let shell_ipc = match ShellIpcService::new().await {
        Ok(service) => Arc::new(service),
        Err(err) => {
            warn!(error = %err, "Shell IPC service unavailable");
            return Err(err.into());
        }
    };

    timer.mark_services_done();

    let services = ShellServices {
        audio: daemons.audio,
        battery: core.battery,
        bluetooth,
        brightness: core.brightness,
        config: config_service,
        hyprland: optional.hyprland,
        power_profiles,
        idle_inhibit: core.idle_inhibit,
        mango: optional.mango,
        media: daemons.media,
        niri: optional.niri,
        network: core.network,
        notification: daemons.notification,
        sysinfo: core.sysinfo,
        systray: daemons.systray,
        wallpaper: core.wallpaper,
        weather,
        shell_ipc,
    };

    Ok((timer, services))
}

async fn init_core_services(
    timer: &StartupTimer,
    config: &wayle_config::Config,
) -> Result<CoreServices, Box<dyn Error>> {
    let modules = &config.modules;

    let theming_monitor = config.styling.theming_monitor.get();
    let theming_monitor = if theming_monitor.is_empty() {
        None
    } else {
        Some(theming_monitor)
    };
    let color_extractor = build_extractor_config(&config.styling);

    let sysinfo = Arc::new(timer.time_sync("Sysinfo", || {
        SysinfoService::builder()
            .cpu_interval(Duration::from_millis(modules.cpu.poll_interval_ms.get()))
            .memory_interval(Duration::from_millis(modules.ram.poll_interval_ms.get()))
            .disk_interval(Duration::from_millis(
                modules.storage.poll_interval_ms.get(),
            ))
            .network_interval(Duration::from_millis(
                modules.netstat.poll_interval_ms.get(),
            ))
            .build()
    }));

    let startup_duration = modules.idle_inhibit.startup_duration.get();

    let battery_task = tokio::spawn(BatteryService::new());
    let brightness_task = tokio::spawn(BrightnessService::new());
    let network_task = tokio::spawn(NetworkService::new());
    let wallpaper_cfg = config.wallpaper.clone();
    let wallpaper_task = tokio::spawn(async move {
        wallpaper::build_wallpaper_service(&wallpaper_cfg, theming_monitor, color_extractor).await
    });
    let idle_inhibit_task = tokio::spawn(IdleInhibitService::new(startup_duration));

    let (battery, brightness, network, wallpaper, idle_inhibit) = tokio::join!(
        async { try_service!(timer, "Battery", spawned(battery_task)) },
        async { try_service!(timer, "Brightness", spawned(brightness_task), no_wrap) },
        async { try_service!(timer, "Network", spawned(network_task)) },
        async { try_service!(timer, "Wallpaper", spawned(wallpaper_task), no_wrap) },
        timer.time("IdleInhibit", spawned(idle_inhibit_task)),
    );

    Ok(CoreServices {
        battery,
        brightness: brightness.flatten(),
        idle_inhibit: Arc::new(idle_inhibit?),
        network,
        sysinfo,
        wallpaper,
    })
}

async fn init_optional_services(timer: &StartupTimer) -> OptionalServices {
    let hyprland_task = tokio::spawn(HyprlandService::new());
    let mango_task = tokio::spawn(MangoService::new());
    let niri_task = tokio::spawn(NiriService::new());

    let (hyprland, mango, niri) = tokio::join!(
        timer.time("Hyprland", spawned(hyprland_task)),
        timer.time("Mango", spawned(mango_task)),
        timer.time("Niri", spawned(niri_task)),
    );

    OptionalServices {
        hyprland: hyprland.ok(),
        mango: mango.ok(),
        niri: niri.ok(),
    }
}

fn spawn_deferred_bluetooth(property: DeferredService<BluetoothService>) {
    tokio::spawn(async move {
        let start = Instant::now();

        match BluetoothService::new().await {
            Ok(service) => {
                let duration_ms = start.elapsed().as_millis() as u64;
                info!(duration_ms, "Bluetooth ready (deferred)");
                property.replace(Some(Arc::new(service)));
            }
            Err(err) => {
                warn!(error = %err, "Bluetooth unavailable");
            }
        }
    });
}

fn spawn_deferred_power_profiles(property: DeferredService<PowerProfilesService>) {
    tokio::spawn(async move {
        let start = Instant::now();

        match PowerProfilesService::builder().with_daemon().build().await {
            Ok(service) => {
                let duration_ms = start.elapsed().as_millis() as u64;
                info!(duration_ms, "PowerProfiles ready (deferred)");
                property.replace(Some(service));
            }
            Err(err) => {
                warn!(error = %err, "PowerProfiles unavailable");
            }
        }
    });
}

async fn init_daemon_services(
    timer: &StartupTimer,
    modules: &wayle_config::schemas::modules::ModulesConfig,
) -> DaemonServices {
    let ignored = modules.media.players_ignored.get().clone();
    let priority = modules.media.player_priority.get().clone();

    let audio_task = tokio::spawn(AudioService::builder().with_daemon().build());
    let media_task = tokio::spawn(
        MediaService::builder()
            .with_daemon()
            .with_art_cache()
            .ignored_players(ignored)
            .priority_players(priority.clone())
            .build(),
    );
    let blocklist = Property::new(modules.notifications.blocklist.get());
    let notification_task = tokio::spawn(
        NotificationService::builder()
            .with_daemon()
            .blocklist(blocklist)
            .build(),
    );
    let systray_task = tokio::spawn(
        SystemTrayService::builder()
            .with_daemon()
            .mode(TrayMode::Auto)
            .build(),
    );

    let (audio, media, notification, systray) = tokio::join!(
        async { try_service!(timer, "Audio", spawned(audio_task), no_wrap) },
        async { try_service!(timer, "Media", spawned(media_task), no_wrap) },
        async { try_service!(timer, "Notification", spawned(notification_task), no_wrap) },
        async { try_service!(timer, "SystemTray", spawned(systray_task), no_wrap) },
    );

    if let Some(media) = &media {
        spawn_active_media_autoswitch(media.clone(), priority);
    }

    DaemonServices {
        audio,
        media,
        notification,
        systray,
    }
}

/// Auto-switch the active media player to whichever player starts playing.
///
/// `wayle-media` only recomputes the active player when a player appears or
/// disappears on the bus, never when an existing player's playback state
/// changes. So pausing one app and playing another leaves the bar stuck on the
/// first. This watches every player's playback state and, whenever the active
/// player is not playing, promotes the best playing player (honoring the same
/// priority patterns the service uses).
fn spawn_active_media_autoswitch(media: Arc<MediaService>, priority: Vec<String>) {
    use futures::StreamExt;

    tokio::spawn(async move {
        loop {
            let players = media.players();
            reevaluate_active_player(&media, &priority).await;

            // players_monitored() emits the current list first; drop it so the
            // inner loop only breaks on real list changes.
            let mut list = Box::pin(media.players_monitored());
            let _ = list.next().await;

            if players.is_empty() {
                if list.next().await.is_none() {
                    break;
                }
                continue;
            }

            // Subscribe once per player-set. Each watch() emits its current
            // value first (a harmless extra re-evaluation), then only on change.
            // Holding `players` keeps the Properties alive so these never end
            // until the next list change rebuilds them.
            let mut playback = futures::stream::select_all(
                players
                    .iter()
                    .map(|p| p.playback_state.watch().map(|_| ()).boxed())
                    .collect::<Vec<_>>(),
            );

            loop {
                tokio::select! {
                    changed = list.next() => {
                        if changed.is_none() {
                            return;
                        }
                        break;
                    }
                    _ = playback.next() => {
                        reevaluate_active_player(&media, &priority).await;
                    }
                }
            }
        }
    });
}

/// If the active player is not playing, promote the best playing player.
///
/// Leaves a playing active player alone so a manual selection sticks; only
/// "rescues" from a paused/stopped/absent active player.
// ponytail: last-playing-wins when active is idle; does not re-rank by priority
// while the active player is still playing (matches the "switch to what I just
// started" expectation). Tighten here if priority pre-emption is ever wanted.
async fn reevaluate_active_player(media: &Arc<MediaService>, priority: &[String]) {
    let active = media.active_player();
    if active
        .as_ref()
        .is_some_and(|a| a.playback_state.get() == PlaybackState::Playing)
    {
        return;
    }

    let players = media.players();
    let playing: Vec<Arc<Player>> = players
        .into_iter()
        .filter(|p| p.playback_state.get() == PlaybackState::Playing)
        .collect();

    let candidate = priority
        .iter()
        .find_map(|pattern| {
            playing
                .iter()
                .find(|p| crate::glob::matches(pattern, p.id.bus_name()))
                .cloned()
        })
        .or_else(|| playing.first().cloned());

    let Some(candidate) = candidate else {
        return;
    };

    if active.as_ref().is_none_or(|a| a.id != candidate.id)
        && let Err(e) = media.set_active_player(Some(candidate.id.clone())).await
    {
        debug!("auto-switch active player failed: {e}");
    }
}
