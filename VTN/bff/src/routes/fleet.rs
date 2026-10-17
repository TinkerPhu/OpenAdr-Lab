//! Serving the fleet's state — what it is doing now, and what it did.
//!
//! One route, two questions, because they are the same question asked at two
//! times: `GET /api/fleet/power` with no window answers "now" from the live
//! subscription, and with a `from` answers "then" from the stored telemetry.
//! Keeping them on one path means a caller that wants both does not have to
//! know that the BFF holds them in two places.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::json;

use crate::fleet_store;
use crate::AppCtx;

/// How wide a bucket a historical query gets when it does not say.
const DEFAULT_STEP_S: i64 = 60;

/// The smallest bucket we will serve. Below the publish cadence the grid says
/// more about the sampling than about the site, and a caller asking for
/// one-second buckets over a day would get 86 400 of them.
const MIN_STEP_S: i64 = 5;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PowerQuery {
    /// Start of the window. Its presence is what makes this a history query.
    pub from: Option<DateTime<Utc>>,
    /// End of the window; defaults to now.
    pub to: Option<DateTime<Utc>>,
    /// Bucket width in seconds.
    pub step_seconds: Option<i64>,
}

/// `GET /api/fleet/power` — every VEN's power, now or over a window.
pub async fn fleet_power(
    State(ctx): State<AppCtx>,
    Query(q): Query<PowerQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match q.from {
        Some(from) => history(ctx, from, q).await,
        None => Ok(live(ctx).await),
    }
}

/// The latest reading from every VEN, and the fleet sum.
///
/// The sum is over what each VEN reported, signed: import positive, export
/// negative. A VEN that has said nothing contributes nothing and is listed
/// with a null value rather than a zero — "we have not heard from it" and
/// "it is drawing nothing" are different facts, and a fleet total that
/// silently treats the first as the second is wrong in exactly the way that
/// is hardest to notice.
async fn live(ctx: AppCtx) -> Json<serde_json::Value> {
    let state = ctx.fleet.read().await;

    let mut vens = Vec::with_capacity(state.len());
    let mut sum_kw = 0.0_f64;
    let mut contributing = 0usize;

    for (name, live) in state.iter() {
        let net_power_kw = live.telemetry.as_ref().and_then(crate::fleet::net_power_kw);
        if let Some(kw) = net_power_kw {
            sum_kw += kw;
            contributing += 1;
        }
        vens.push(json!({
            "venName": name,
            "netPowerKw": net_power_kw,
            "state": live.state,
            "receivedAt": live.received_at,
            // The VEN's own `objective` body and the fleet's command, as the broker holds them.
            "objective": live.objective,
            "command": live.command,
            "commandPending": live.command_pending(),
        }));
    }

    Json(json!({
        "source": "live",
        "vens": vens,
        "fleet": {
            "netPowerKw": sum_kw,
            // The sum is only as complete as the VENs behind it. Saying how
            // many contributed lets a reader judge it rather than trust it.
            "contributingVens": contributing,
            "knownVens": state.len(),
        }
    }))
}

/// Per-VEN series over a window, plus the fleet sum on the same grid.
async fn history(
    ctx: AppCtx,
    from: DateTime<Utc>,
    q: PowerQuery,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let now = Utc::now();
    let to = q.to.unwrap_or(now);
    if to <= from {
        return Err(bad_request("`to` must be after `from`"));
    }
    let step = Duration::seconds(q.step_seconds.unwrap_or(DEFAULT_STEP_S).max(MIN_STEP_S));

    // "Not connected yet" is not "nothing happened then". Answering an empty
    // series here would be indistinguishable from a quiet fleet, and only one
    // of the two is worth retrying.
    let pool = ctx.fleet_store.read().await.clone();
    let Some(pool) = pool else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "fleet telemetry store is not connected"})),
        ));
    };

    let (source, series) = fleet_store::power_series(&pool, from, to, step, now)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "fleet power query failed");
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error": "fleet telemetry query failed"})),
            )
        })?;

    let vens: Vec<_> = series
        .iter()
        .map(|(name, s)| {
            json!({
                "venName": name,
                "samples": s.samples.iter()
                    .map(|(ts, v)| json!({"ts": ts, "netPowerKw": v}))
                    .collect::<Vec<_>>(),
            })
        })
        .collect();

    let fleet: Vec<_> = fleet_store::sum_over_grid(&series)
        .into_iter()
        .map(|(ts, sum, n)| json!({"ts": ts, "netPowerKw": sum, "contributingVens": n}))
        .collect();

    Ok(Json(json!({
        // Which table answered. A 1-minute rollup and a 5-second raw series
        // are both true and not the same resolution; a reader comparing two
        // windows needs to be told which they have.
        "source": source,
        "from": from,
        "to": to,
        "stepSeconds": step.num_seconds(),
        "vens": vens,
        "fleet": fleet,
    })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReactionQuery {
    #[serde(rename = "eventID")]
    pub event_id: String,
}

/// `GET /api/fleet/reactions?eventID=…` — who saw one event, and what changed.
///
/// `eventID` spelled as OpenADR spells it (`dto`): the same word a caller read
/// off the event object is the word they pass here.
pub async fn fleet_reactions(
    State(ctx): State<AppCtx>,
    Query(q): Query<ReactionQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if q.event_id.trim().is_empty() {
        return Err(bad_request("eventID is required"));
    }
    let pool = ctx.fleet_store.read().await.clone();
    let Some(pool) = pool else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "fleet telemetry store is not connected"})),
        ));
    };

    let reactions = crate::fleet_reactions::reactions(&pool, &q.event_id)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "fleet reactions query failed");
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error": "fleet reactions query failed"})),
            )
        })?;

    let vens: Vec<_> = reactions
        .iter()
        .map(|r| {
            let mut v = serde_json::to_value(r).unwrap_or_else(|_| json!({}));
            if let Some(obj) = v.as_object_mut() {
                obj.insert("deltaKw".into(), json!(r.delta_kw()));
            }
            v
        })
        .collect();

    Ok(Json(json!({
        "eventID": q.event_id,
        // How many VENs said they saw it. The list holds only those, so this
        // is a count of evidence rather than of the fleet.
        "vensSeen": vens.len(),
        "vens": vens,
    })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignalsQuery {
    pub from: DateTime<Utc>,
    pub to: Option<DateTime<Utc>>,
}

/// `GET /api/fleet/signals?from&to` — what each VEN was being told, and when.
///
/// The companion to `/api/fleet/power`: that says what a site did, this says
/// what it was asked to do. Drawn together, a dip at 09:15 stops being
/// ambiguous between "a limit landed" and "somebody boiled a kettle".
///
/// Resolved with `lab-core`'s interval rule — the same one the VEN plans
/// against, so a band drawn here and the limit actually applied there cannot
/// disagree about when interval *i* runs.
pub async fn fleet_signals(
    State(ctx): State<AppCtx>,
    Query(q): Query<SignalsQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let to = q.to.unwrap_or_else(Utc::now);
    if to <= q.from {
        return Err(bad_request("`to` must be after `from`"));
    }

    let raw = ctx
        .business
        .get_all_pages("/events", None)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "fleet signals: could not list events");
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error": "could not read events from the VTN"})),
            )
        })?;

    // One malformed event costs only itself. The alternative — failing the
    // whole request — turns one bad object into "the fleet had no signals",
    // which is the more dangerous answer because it looks like a quiet grid.
    let mut events = Vec::new();
    let mut rejected = 0usize;
    for row in raw {
        match serde_json::from_value::<lab_core::event_timing::OadrEvent>(row) {
            Ok(ev) => events.push(ev),
            Err(e) => {
                rejected += 1;
                tracing::warn!(error = %e, "fleet signals: unreadable event skipped");
            }
        }
    }

    // GB-50: a band's value is read the way a VEN reads it, through what the event and its
    // program declare. Without the programs the reader still has each event's own descriptors
    // and the profile default, so a failed listing costs the program tier, not the bands.
    let programs = ctx
        .business
        .get_all_pages("/programs", None)
        .await
        .unwrap_or_else(|e| {
            tracing::warn!(error = %e, "fleet signals: could not list programs; reading without them");
            Vec::new()
        });
    let reader = lab_core::wire_contract::PayloadReader::from_program_rows(&programs);

    // Every VEN the feed knows about, so a targeted VEN that has published
    // nothing still shows its bands rather than vanishing.
    let ven_names: Vec<String> = ctx.fleet.read().await.keys().cloned().collect();

    let vens: Vec<_> = ven_names
        .iter()
        .map(|name| {
            let bands: Vec<_> = events
                .iter()
                .flat_map(|ev| crate::fleet_signals::bands_for_ven(ev, &reader, name, q.from, to))
                .collect();
            json!({ "venName": name, "bands": bands })
        })
        .collect();

    Ok(Json(json!({
        "from": q.from,
        "to": to,
        "vens": vens,
        // Said out loud rather than folded into an empty result: an operator
        // seeing fewer bands than expected needs to know whether the VTN sent
        // something this BFF could not read.
        "rejectedEvents": rejected,
    })))
}

fn bad_request(message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::BAD_REQUEST, Json(json!({"error": message})))
}

/// The body of both objective commands: an objective's name, or `null` to release the fleet's
/// command (the VEN goes back to its profile objective).
#[derive(Debug, Deserialize)]
pub struct ObjectiveBody {
    pub objective: Option<String>,
}

fn refused(r: crate::fleet_command::CommandRefused) -> (StatusCode, Json<serde_json::Value>) {
    use crate::fleet_command::CommandRefused::*;
    let status = match r {
        UnknownVen(_) => StatusCode::NOT_FOUND,
        ObjectivesUnknown(_) => StatusCode::CONFLICT,
        NotAccepted { .. } => StatusCode::BAD_REQUEST,
    };
    (status, Json(json!({"error": r.message()})))
}

fn commands(
    ctx: &AppCtx,
) -> Result<
    std::sync::Arc<dyn crate::fleet_command::CommandPublisher>,
    (StatusCode, Json<serde_json::Value>),
> {
    ctx.fleet_commands.clone().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"error": "no fleet broker is configured"})),
    ))
}

/// `PUT /api/fleet/vens/:ven/objective` — command one VEN's objective, or release it.
pub async fn put_ven_objective(
    State(ctx): State<AppCtx>,
    axum::extract::Path(ven): axum::extract::Path<String>,
    Json(body): Json<ObjectiveBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let publisher = commands(&ctx)?;
    let objective = body.objective.as_deref();
    crate::fleet_command::validate(&*ctx.fleet.read().await, &ven, objective).map_err(refused)?;
    crate::fleet_command::command_ven(
        publisher.as_ref(),
        &ctx.fleet_root,
        &ven,
        objective,
        Utc::now(),
    )
    .map_err(|e| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": format!("command not published: {e}")})),
        )
    })?;
    Ok(Json(json!({"venName": ven, "objective": body.objective})))
}

/// `PUT /api/fleet/objective` — command every VEN, one at a time
/// (`FLEET_OBJECTIVE_SPACING_S` apart); all or nothing on validation.
pub async fn put_fleet_objective(
    State(ctx): State<AppCtx>,
    Json(body): Json<ObjectiveBody>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    let publisher = commands(&ctx)?;
    let (targets, skipped) =
        crate::fleet_command::fleet_targets(&*ctx.fleet.read().await, body.objective.as_deref())
            .map_err(refused)?;
    let spacing_s = ctx.config.fleet_objective_spacing_s;
    crate::fleet_command::command_fleet(
        publisher,
        ctx.fleet_root.clone(),
        targets.clone(),
        body.objective.clone(),
        std::time::Duration::from_secs(spacing_s),
    );
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({
            "objective": body.objective,
            "vens": targets,
            // Known VENs that have not said which objectives they accept, so were not commanded.
            "skipped": skipped,
            "spacingSeconds": spacing_s,
        })),
    ))
}

/// `GET /api/fleet/vens/:ven/plan` — the plan the VEN adopted last, as it published it.
pub async fn get_ven_plan(
    State(ctx): State<AppCtx>,
    axum::extract::Path(ven): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let fleet = ctx.fleet.read().await;
    match fleet.get(&ven).and_then(|v| v.plan.clone()) {
        Some(plan) => Ok(Json(plan)),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": format!("VEN `{ven}` has published no plan")})),
        )),
    }
}
