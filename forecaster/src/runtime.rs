//! The forecaster's contact with the world: the broker connection, the clock and the shutdown signal.
//!
//! Owns: the MQTT session (last will, subscriptions, retained publishes), the periodic tick and the clean
//! shutdown. Reads: the environment-derived `Config`, the profile text, the broker. Must never: decide
//! anything about forecasts; `dispatch` and `engine` do, and are tested without this file.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use lab_forecast::forecast_payload::{ForecastStatus, ForecastTopics};
use lab_forecast::pv_profile::{base_load_measured_from_yaml, pv_arrays_from_yaml};
use rumqttc::{AsyncClient, Event, EventLoop, LastWill, MqttOptions, Packet, QoS};
use tracing::{info, warn};

use crate::cache;
use crate::config::Config;
use crate::dispatch::{check_single_producer, handle_message, handle_tick, FeedTopics};
use crate::engine::{Engine, EngineSettings, Publication};

/// Where "now" comes from; `Utc::now` in the container, a fixed value in a test.
pub type Clock = Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>;

/// How long to listen for another producer's retained status before publishing anything.
const GUARD_WAIT: Duration = Duration::from_secs(3);
/// How long to wait for the first connection before giving up (the container restarts and tries again).
const CONNECT_PATIENCE: Duration = Duration::from_secs(60);
/// The weather forecast is a few KB per day; a forecast payload of 48 hours is far below this.
const MAX_PACKET_BYTES: usize = 256 * 1024;

/// Run until a shutdown signal; an error is a reason to exit non-zero.
pub async fn run(config: Config, profile_yaml: &str, clock: Clock) -> Result<(), String> {
    let arrays = pv_arrays_from_yaml(profile_yaml)?;
    let topics = ForecastTopics::new(&config.root, &config.site).map_err(|e| e.to_string())?;
    let base_load_enabled = base_load_measured_from_yaml(profile_yaml)?;
    let feeds = FeedTopics::new(
        &config.root,
        &config.site,
        arrays.iter().any(|a| a.measured),
        base_load_enabled,
    );
    info!(
        arrays = arrays.len(),
        site = %config.site,
        producer = %config.producer_id,
        "forecaster starting"
    );
    let mut engine = Engine::new(EngineSettings {
        topics: topics.clone(),
        arrays,
        producer_id: config.producer_id.clone(),
        model_version: config.model_version.clone(),
        freshness: config.freshness,
        base_load_enabled,
    });
    load_cache(&mut engine, config.cache_path.as_deref());

    let (client, mut eventloop) = AsyncClient::new(mqtt_options(&config, &topics, clock()), 64);
    guard_single_producer(&client, &mut eventloop, &topics, &config, &clock).await?;
    subscribe_feeds(&client, &feeds).await;
    publish_all(&client, handle_tick(&mut engine, clock()));

    let mut tick = tokio::time::interval(config.recompute_every);
    tick.tick().await; // the first tick fires at once; the start-up publish above covered it
    loop {
        tokio::select! {
            event = eventloop.poll() => match event {
                Ok(Event::Incoming(Packet::ConnAck(_))) => subscribe_feeds(&client, &feeds).await,
                Ok(Event::Incoming(Packet::Publish(p))) => {
                    let handled = handle_message(&mut engine, &feeds, &p.topic, &p.payload, clock());
                    if let Some(reason) = handled.rejected {
                        warn!(topic = %p.topic, "{reason}");
                    }
                    publish_all(&client, handled.publications);
                    log_problem(&engine);
                }
                Ok(_) => {}
                Err(e) => {
                    warn!(error = %e, "broker connection error, retrying");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            },
            _ = tick.tick() => {
                publish_all(&client, handle_tick(&mut engine, clock()));
                log_problem(&engine);
                save_cache(&engine, config.cache_path.as_deref());
            }
            _ = shutdown_signal() => {
                info!("shutting down: publishing offline");
                save_cache(&engine, config.cache_path.as_deref());
                say_goodbye(&client, &mut eventloop, &topics, &config, clock()).await;
                return Ok(());
            }
        }
    }
}

fn mqtt_options(config: &Config, topics: &ForecastTopics, now: DateTime<Utc>) -> MqttOptions {
    let mut opts = MqttOptions::new(
        format!("lab-forecaster-{}", config.producer_id),
        config.mqtt_host.clone(),
        config.mqtt_port,
    );
    opts.set_keep_alive(Duration::from_secs(30));
    opts.set_max_packet_size(MAX_PACKET_BYTES, MAX_PACKET_BYTES);
    if let (Some(user), Some(password)) = (&config.mqtt_username, &config.mqtt_password) {
        opts.set_credentials(user.clone(), password.clone());
    }
    // If this container dies without saying goodbye, the broker says it for us.
    opts.set_last_will(LastWill::new(
        topics.status(),
        offline_status(config, now),
        QoS::AtLeastOnce,
        true,
    ));
    opts
}

fn offline_status(config: &Config, now: DateTime<Utc>) -> Vec<u8> {
    serde_json::to_vec(&ForecastStatus::offline(&config.producer_id, now))
        .expect("a status always serialises")
}

/// Connect, read what is retained on our status topic for a moment, and refuse to go on when another
/// producer is live there (nothing has been published by then).
async fn guard_single_producer(
    client: &AsyncClient,
    eventloop: &mut EventLoop,
    topics: &ForecastTopics,
    config: &Config,
    clock: &Clock,
) -> Result<(), String> {
    let status_topic = topics.status();
    let mut retained: Option<Vec<u8>> = None;
    let started = tokio::time::Instant::now();
    let mut connected_at: Option<tokio::time::Instant> = None;
    loop {
        let wait_until = match connected_at {
            Some(t) => t + GUARD_WAIT,
            None => started + CONNECT_PATIENCE,
        };
        match tokio::time::timeout_at(wait_until, eventloop.poll()).await {
            Err(_) if connected_at.is_some() => break, // the listening window is over
            Err(_) => return Err("cannot reach the broker".to_string()),
            Ok(Ok(Event::Incoming(Packet::ConnAck(_)))) => {
                connected_at = Some(tokio::time::Instant::now());
                client
                    .subscribe(&status_topic, QoS::AtLeastOnce)
                    .await
                    .map_err(|e| format!("cannot subscribe to {status_topic}: {e}"))?;
            }
            Ok(Ok(Event::Incoming(Packet::Publish(p)))) if p.topic == status_topic => {
                retained = Some(p.payload.to_vec());
            }
            Ok(Ok(_)) => {}
            Ok(Err(e)) => {
                warn!(error = %e, "broker not reachable yet, retrying");
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
    check_single_producer(
        retained.as_deref(),
        &config.producer_id,
        clock(),
        config.freshness,
    )
}

async fn subscribe_feeds(client: &AsyncClient, feeds: &FeedTopics) {
    let wanted = std::iter::once(&feeds.weather_forecast)
        .chain(feeds.pv_measurement.as_ref())
        .chain(feeds.base_load_measurement.as_ref());
    for topic in wanted {
        if let Err(e) = client.subscribe(topic, QoS::AtLeastOnce).await {
            warn!(error = %e, topic = %topic, "subscribe failed");
        }
    }
}

/// Retained, QoS 1. `try_publish`, never an await: this runs on the task that drives the connection, and
/// an await there would wait for the loop that is waiting for it.
fn publish_all(client: &AsyncClient, publications: Vec<Publication>) {
    for p in publications {
        if let Err(e) = client.try_publish(&p.topic, QoS::AtLeastOnce, true, p.payload) {
            warn!(error = %e, topic = %p.topic, "publish dropped");
        }
    }
}

/// Put the saved base-load history back. A missing or damaged file costs only learned history.
fn load_cache(engine: &mut Engine, path: Option<&str>) {
    let Some(path) = path else { return };
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let (samples, skipped) = cache::decode(&text);
            info!(
                samples = samples.len(),
                skipped, path, "base-load history restored"
            );
            engine.restore_base_load_history(&samples);
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            info!(path, "no base-load cache yet: starting from cold");
        }
        Err(e) => warn!(error = %e, path, "base-load cache unreadable: starting from cold"),
    }
}

/// Write the history next to the cache file and rename it over, so a crash never leaves half a file.
fn save_cache(engine: &Engine, path: Option<&str>) {
    let Some(path) = path else { return };
    let history = engine.base_load_history();
    if history.is_empty() {
        return;
    }
    let tmp = format!("{path}.tmp");
    let written =
        std::fs::write(&tmp, cache::encode(&history)).and_then(|()| std::fs::rename(&tmp, path));
    if let Err(e) = written {
        warn!(error = %e, path, "base-load cache not saved");
    }
}

fn log_problem(engine: &Engine) {
    if let Some(problem) = engine.last_problem() {
        warn!(state = ?engine.state(), "{problem}");
    }
}

async fn say_goodbye(
    client: &AsyncClient,
    eventloop: &mut EventLoop,
    topics: &ForecastTopics,
    config: &Config,
    now: DateTime<Utc>,
) {
    if client
        .try_publish(
            topics.status(),
            QoS::AtLeastOnce,
            true,
            offline_status(config, now),
        )
        .is_err()
        || client.try_disconnect().is_err()
    {
        return;
    }
    // Drive the loop until the disconnect has gone out, but not for long.
    let _ = tokio::time::timeout(Duration::from_secs(3), async {
        while let Ok(event) = eventloop.poll().await {
            if matches!(event, Event::Outgoing(rumqttc::Outgoing::Disconnect)) {
                break;
            }
        }
    })
    .await;
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => {
                tokio::select! {
                    _ = term.recv() => {}
                    _ = tokio::signal::ctrl_c() => {}
                }
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
