//! Submitting a report to the VTN, and remembering what it was called.
//!
//! Split out of `vtn.rs` (R-40 "split proactively when next touched") once
//! that file crossed the `VEN/src/` 500-production-line cap. Same split-`impl`
//! pattern `state/` uses; `VtnClient` stays one type.
//!
//! The shape of the problem: a `reportName` is stable per event and
//! obligation, so *every* periodic submission after the first necessarily
//! collides. The VTN answers 409, and the old recovery was to list our own
//! reports (paginated) to find the id we had already been told once -- three
//! requests, every five minutes, per VEN, growing with the report count.
//!
//! So the id is remembered. The cache is an optimisation and nothing else:
//! every entry is re-derivable from the VTN, and an entry that has gone stale
//! costs one rejected PUT before the create path re-learns it. Correctness
//! never depends on it being right.

use anyhow::{Context, Result};
use reqwest::StatusCode;

use crate::controller::VtnPort;
use crate::vtn::{describe_problem, http_error, VtnClient};

impl VtnClient {
    /// Submit a report with upsert semantics.
    ///
    /// Steady state is a PUT to a remembered id (F-6). Before that cache
    /// existed every periodic submission cost three requests -- POST, the 409
    /// that always follows a stable `reportName`, a paginated lookup of our own
    /// reports, then the PUT -- which at 20 VENs was ~120 requests per five
    /// minutes and grew with the report count.
    ///
    /// The cache is an optimisation only. A remembered id that the VTN no
    /// longer has (report deleted, database reset) simply falls through to the
    /// POST path, which re-learns it: correctness never depends on the cache
    /// being right, only speed.
    pub(crate) async fn upsert_report(
        &self,
        body: crate::controller::vtn_port::OadrReportBody,
    ) -> Result<()> {
        let value = serde_json::to_value(&body).context("serialize report body")?;

        if let Some(name) = body.reportName.as_deref() {
            let cached = self.report_ids.read().await.get(name).cloned();
            if let Some(id) = cached {
                match self.update_report(&id, value.clone()).await {
                    Ok(_) => return Ok(()),
                    Err(e) => {
                        // Stale id. Forget it and fall through to create.
                        self.report_ids.write().await.remove(name);
                        tracing::debug!(
                            report_name = name,
                            report_id = %id,
                            error = %e,
                            "cached report id rejected; re-creating"
                        );
                    }
                }
            }
        }

        let (status, text) = self.post_json_raw("/reports", &value).await?;

        if status.is_success() {
            // Remember what the VTN called it, so the next submission is one
            // request instead of three.
            if let (Some(name), Some(id)) = (
                body.reportName.as_deref(),
                serde_json::from_str::<serde_json::Value>(&text)
                    .ok()
                    .and_then(|v| v.get("id")?.as_str().map(str::to_string)),
            ) {
                self.report_ids.write().await.insert(name.to_string(), id);
            }
            return Ok(());
        }

        if status == StatusCode::CONFLICT {
            // A VTN 409 here is the expected, steady-state outcome for every
            // periodic report submission after the first (reporter.rs gives
            // each report a name stable per event/obligation, so re-submitting
            // it necessarily 409s and falls through to a name-based PUT) — it
            // is not proof of a reportName duplicate either way: openleadr-rs
            // also maps foreign-key violations (e.g. the referenced event was
            // cascade-deleted) to 409. So this branch must NOT log at ERROR
            // until it knows recovery actually failed — see `describe_problem`.
            let vtn_problem = describe_problem(&text);
            if let Some(name) = body.reportName.as_deref() {
                match self.find_report_by_name(name).await {
                    Ok(id) => {
                        self.update_report(&id, value).await?;
                        // Learn it here too: this is the path a VEN takes for a
                        // report that already existed before it started (a
                        // restart, or a report created by a previous process).
                        self.report_ids
                            .write()
                            .await
                            .insert(name.to_string(), id.clone());
                        tracing::debug!(
                            path = "/reports",
                            report_name = name,
                            "409 on POST /reports resolved via name-based upsert"
                        );
                        return Ok(());
                    }
                    Err(lookup_err) => {
                        tracing::error!(
                            path = "/reports",
                            status = status.as_u16(),
                            report_name = name,
                            vtn_problem,
                            "409 on POST /reports and name-based upsert failed"
                        );
                        anyhow::bail!(
                            "409 on POST /reports and upsert of reportName '{name}' failed \
                             ({lookup_err:#}); VTN said: {vtn_problem}"
                        )
                    }
                }
            }
            tracing::error!(
                path = "/reports",
                status = status.as_u16(),
                vtn_problem,
                "409 on POST /reports without reportName — cannot upsert by name"
            );
            anyhow::bail!(
                "409 on POST /reports without reportName — cannot upsert by name; \
                 VTN said: {vtn_problem}"
            );
        }

        if !status.is_success() {
            return Err(http_error("/reports", status, &text));
        }

        Ok(())
    }

    pub(crate) async fn update_report(
        &self,
        id: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let path = format!("/reports/{id}");
        self.put_json(&path, body).await
    }

    /// Search own reports (filtered by client_name) for a matching reportName.
    async fn find_report_by_name(&self, report_name: &str) -> Result<String> {
        let reports = VtnPort::fetch_reports(self).await?;
        for r in &reports.items {
            if r.reportName.as_deref() == Some(report_name) {
                return Ok(r.id.clone());
            }
        }
        anyhow::bail!("no report found with name '{report_name}'")
    }
}
