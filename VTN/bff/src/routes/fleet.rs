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
    let mut sum_w = 0.0_f64;
    let mut contributing = 0usize;

    for (name, live) in state.iter() {
        let net_power_w = live.telemetry.as_ref().and_then(crate::fleet::net_power_w);
        if let Some(w) = net_power_w {
            sum_w += w;
            contributing += 1;
        }
        vens.push(json!({
            "venName": name,
            "netPowerW": net_power_w,
            "state": live.state,
            "receivedAt": live.received_at,
        }));
    }

    Json(json!({
        "source": "live",
        "vens": vens,
        "fleet": {
            "netPowerW": sum_w,
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
                    .map(|(ts, v)| json!({"ts": ts, "netPowerW": v}))
                    .collect::<Vec<_>>(),
            })
        })
        .collect();

    let fleet: Vec<_> = fleet_store::sum_over_grid(&series)
        .into_iter()
        .map(|(ts, sum, n)| json!({"ts": ts, "netPowerW": sum, "contributingVens": n}))
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
                obj.insert("deltaW".into(), json!(r.delta_w()));
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

fn bad_request(message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::BAD_REQUEST, Json(json!({"error": message})))
}
