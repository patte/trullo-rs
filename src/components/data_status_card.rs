use dioxus::prelude::*;

use crate::api::{get_scheduler_status, latest_data_status};
use crate::components::Gauge;
use crate::utils::format::{format_local, format_megabytes};

#[allow(non_snake_case)]
#[component]
pub fn DataStatusCard() -> Element {
    let latest = use_resource(|| async move { latest_data_status().await.ok().flatten() });
    let status = use_resource(|| async move { get_scheduler_status().await.ok() });
    rsx! {
            // Card
            div { class: "w-full rounded-2xl border border-slate-800 bg-slate-900/60 backdrop-blur-sm shadow-xl p-8 space-y-6",
                h1 { class: "text-2xl font-semibold tracking-tight text-slate-200", "WindTre Data Status" }
                {
                    let data = latest.read_unchecked();
                    match &*data {
                        Some(Some(ds)) => rsx! {
                            div { class: "flex flex-col items-center gap-3",
                                // Gauge + percentage
                                Gauge {
                                    value: ds.remaining_percentage,
                                    start_angle: 45.0,
                                    stop_angle: 315.0,
                                    size: 220,
                                    stroke: 14,
                                    track_class: "text-slate-800".to_string(),
                                    progress_class: "text-emerald-400".to_string(),
                                    div { class: "text-5xl font-bold text-emerald-400 tabular-nums", "{ds.remaining_percentage}%" }
                                }
                                // MB remaining
                                div { class: "text-lg text-slate-300", "{format_megabytes(ds.remaining_data_mb)} remaining" }
                                // Timestamp formatted in local time
                                div { class: "text-xs text-slate-400", "As of {format_local(&ds.date_time)}" }
                            }
                        },
                        Some(None) => rsx! {
                            div { class: "text-center text-slate-300",
                                p { class: "text-lg", "No data yet" }
                                p { class: "text-sm text-slate-400", "Awaiting SMS update from the router..." }
                            }
                        },
                        None => rsx! {
                            div { class: "animate-pulse space-y-3",
                                div { class: "h-9 w-28 bg-slate-800 rounded" }
                                div { class: "h-5 w-48 bg-slate-800 rounded" }
                                div { class: "h-3 w-40 bg-slate-800 rounded" }
                            }
                        },
                    }
                }
                // Diagnostics
                {
                    match &*status.read_unchecked() {
                        Some(Some(st)) => rsx!{
                            div { class: "pt-2 border-t border-slate-800 text-xs text-slate-400 space-y-1",
                                if let Some(err) = &st.last_error { div { class: "text-red-400 text-sm font-medium", "Error: {err}" } }
                                if let Some(ev) = &st.last_event { div { "Status: {ev}" } }
                                if let Some(ts) = &st.last_loop_at { div { "Last loop: {format_local(ts)}" } }
                                // div { class: "truncate", "DB: {st.db_url}" }
                            }
                        },
                        Some(None) => rsx!{ div { class: "text-xs text-slate-500", "No status yet..." } },
                        None => rsx!{ div { class: "text-xs text-slate-500", "Loading status..." } },
                    }
                }
            }
    }
}
