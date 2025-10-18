use dioxus::prelude::*;

#[cfg(feature = "web")]
use dioxus::logger::tracing::info;

use crate::api::{get_scheduler_status, latest_data_status};
use crate::components::Gauge;
use crate::utils::format::{format_local, format_megabytes};

#[allow(non_snake_case)]
#[component]
pub fn DataStatusCard() -> Element {
    // ssr data (server waits)
    let latest = use_server_future(latest_data_status)?;
    let latest_v = latest.read_unchecked();

    // Client-only fetch
    let status = use_resource(|| async move { get_scheduler_status().await.ok() });
    let status_v = status.read_unchecked();

    // Force one rerender after hydration so client formatting can apply
    let hydrated = use_signal(|| false);
    #[cfg(feature = "web")]
    {
        use_effect({
            let mut hydrated = hydrated.clone();
            move || {
                hydrated.set(true); // runs once on the client after hydration
            }
        });
    }

    // ---------- NEW: client-side timer effect ----------
    #[cfg(feature = "web")]
    {
        use gloo_timers::callback::{Interval, Timeout};
        use js_sys::Date;
        use wasm_bindgen::JsValue;

        // Keep handles so we can cancel them on re-runs/unmount
        let timer_handle: Signal<Option<Timeout>> = use_signal(|| None);

        // teardown on unmount
        use_drop({
            let mut timer_handle = timer_handle.clone();
            move || {
                if let Some(h) = timer_handle.write().take() {
                    h.cancel();
                }
            }
        });

        // refresh at next_iteration_at or poll every second
        use_effect({
            let mut status = status.clone();
            let mut timer_handle = timer_handle.clone();

            move || {
                // Read resource *inside* the effect so it re-runs after restart()
                let next_iso = status
                    .read()
                    .as_ref()
                    .and_then(|s| s.as_ref())
                    .and_then(|st| st.next_iteration_at.clone());

                // Cancel any previous timeout before scheduling a new one
                if let Some(prev) = timer_handle.write().take() {
                    prev.cancel();
                }

                // Decide schedule based on next_iso:
                // - Some future ts  -> one-shot to exact instant
                // - Some past ts    -> 1s polling
                // - None            -> 1s polling
                let delay_ms = if let Some(next_str) = next_iso {
                    let target_ms = Date::new(&JsValue::from_str(&next_str)).get_time();
                    let now_ms = Date::now();
                    if target_ms.is_finite() && target_ms > now_ms {
                        (target_ms - now_ms) as u32
                    } else {
                        // Past or unparsable -> poll
                        1_000
                    }
                } else {
                    // No next_iteration_at -> poll
                    1_000
                };

                info!(
                    "[data_status_card] scheduling next fetch in {} ms",
                    delay_ms
                );

                // Schedule the timeout (one-shot or poll tick)
                let handle = Timeout::new(delay_ms, move || {
                    status.restart(); // effect will re-run after the fetch settles
                });
                timer_handle.set(Some(handle));
            }
        });
    }

    rsx! {
        // Card
        div { class: "w-full rounded-2xl border border-slate-800 bg-slate-900/60 backdrop-blur-sm shadow-xl p-8 space-y-6",
            h1 { class: "text-2xl font-semibold tracking-tight text-slate-200", "WindTre Data Status" }

            {
                match &*latest_v {
                    // Data available
                    Some(Ok(Some(ds))) => {
                        let shown_time = if *hydrated.read() {
                            format_local(&ds.date_time)
                        } else {
                            ds.date_time.clone()
                        };
                        rsx! {
                        div { class: "flex flex-col items-center gap-3",
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
                            div { class: "text-lg text-slate-300", "{format_megabytes(ds.remaining_data_mb)} remaining" }
                            time { class: "text-xs text-slate-400", datetime: "{ds.date_time}", "As of {shown_time}" }
                        }
                    }},
                    // No data yet
                    Some(Ok(None)) => rsx! {
                        div { class: "text-center text-slate-300",
                            p { class: "text-lg", "No data yet" }
                            p { class: "text-sm text-slate-400", "Awaiting SMS update from the router..." }
                        }
                    },
                    // Server fn error
                    Some(Err(_e)) => rsx! {
                        div { class: "text-center text-slate-300",
                            p { class: "text-lg", "Failed to load status." }
                        }
                    },
                    // Only occurs on client-side navigations (not on first SSR render)
                    None => rsx! {
                        div { class: "animate-pulse space-y-3",
                            div { class: "h-9 w-28 bg-slate-800 rounded" }
                            div { class: "h-5 w-48 bg-slate-800 rounded" }
                            div { class: "h-3 w-40 bg-slate-800 rounded" }
                        }
                    },
                }
            }

            // Diagnostics (only when there's an error)
            {
                match &*status_v {
                    Some(Some(st)) if st.last_error.is_some() => rsx!{
                        div { class: "pt-2 border-t border-slate-800 text-xs text-slate-400 space-y-1",
                            if let Some(err) = &st.last_error { div { class: "text-red-400 text-sm font-medium", "Error: {err}" } }
                            if let Some(ev) = &st.last_event { div { "Status: {ev}" } }
                            if let Some(ts) = &st.last_loop_at { div { "Last loop: {format_local(ts)}" } }
                            if let Some(next_ts) = &st.next_iteration_at { div { "Next run: {format_local(next_ts)}" } }
                        }
                    },
                    _ => rsx!( Fragment {} ),
                }
            }
        }
    }
}
