use dioxus::prelude::*;

use crate::api::get_daily_usage;
use crate::utils::format::{format_megabytes, format_megabytes_f32};

#[allow(non_snake_case)]
#[component]
pub fn UsageChartView() -> Element {
    // Fetch data
    let data = use_resource(|| async move { get_daily_usage().await.ok().unwrap_or_default() });
    let points = data.read_unchecked().clone().unwrap_or_default();
    // Hovered bar index (for tooltip)
    let mut hovered = use_signal(|| Option::<usize>::None);
    // Visual params
    let height = 180.0f32;
    let padding = 20.0f32;
    let bar_gap = 2.0f32;
    let n = points.len().max(1) as f32;
    let width = (n * (6.0 + bar_gap) + padding * 2.0).ceil();
    let max_used = points.iter().map(|p| p.used_mb).max().unwrap_or(1) as f32;
    let view_box = format!("0 0 {} {}", width, height + padding * 2.0);

    // Date formatter for tooltip (yyyy-mm-dd -> dd.mm.yyyy)
    let fmt_date = |s: &str| -> String {
        if s.len() >= 10 {
            format!("{}.{}.{}", &s[8..10], &s[5..7], &s[0..4])
        } else {
            s.to_string()
        }
    };

    // Month label iterator will track seen months internally
    use std::collections::HashSet;

    rsx! {
        div { class: "rounded-2xl border border-slate-800 bg-slate-900/60 backdrop-blur-sm shadow-xl p-6 space-y-3",
            div { class: "flex items-end justify-between",
                h2 { class: "text-lg font-medium text-slate-200", "Daily usage (last 90 days)" }
                if max_used > 0.0 { div { class: "text-xs text-slate-400", "Peak: {format_megabytes_f32(max_used)}" } }
            }
            div { class: "w-full overflow-x-auto",
                svg { class: "block min-w-full", view_box: "{view_box}", width: "100%", height: "{(height + padding*2.0).to_string()}",
                    line { x1: "{padding}", y1: "{padding + height}", x2: "{width - padding}", y2: "{padding + height}", stroke: "#1f2937", stroke_width: "1" }
                    {
                        points.iter().enumerate().map(|(i, p)| {
                            let x = padding + (i as f32) * (6.0 + bar_gap);
                            let h = if max_used <= 0.0 { 0.0 } else { (p.used_mb as f32) / max_used * height };
                            let y = padding + (height - h);
                            let cls = if p.used_mb == 0 { "text-slate-800" } else { "text-emerald-400/80" };
                            rsx!{ rect {
                                key: "{i}", class: "{cls}", x: "{x}", y: "{y}", width: "6", height: "{h}", fill: "currentColor", rx: "2",
                                onmouseenter: move |_| *hovered.write() = Some(i),
                                onmouseleave: move |_| *hovered.write() = None,
                                ontouchstart: move |_| *hovered.write() = Some(i),
                                ontouchend: move |_| *hovered.write() = None,
                            }}
                        })
                    }
                    {
                        match *hovered.read() {
                            Some(i) => {
                                let p = &points[i];
                                let x = padding + (i as f32) * (6.0 + bar_gap) + 3.0; // center of bar
                                let h = if max_used <= 0.0 { 0.0 } else { (p.used_mb as f32) / max_used * height };
                                let y = padding + (height - h);
                                let date_label = fmt_date(&p.date);
                                let value_label = format!("{}", format_megabytes(p.used_mb));
                                let cw = 7.0f32; // approx char width at 11px
                                let content_w = (date_label.len().max(value_label.len()) as f32) * cw + 12.0; // padding
                                let tip_w = content_w.max(12.0).min(width - padding * 2.0);
                                let tip_h = 36.0f32; // two lines
                                let tip_x = (x - tip_w / 2.0).clamp(padding, (width - padding) - tip_w);
                                let tip_y = (y - 10.0 - tip_h).max(6.0);
                                rsx!{ g { key: "tooltip",
                                    line { x1: "{x}", y1: "{y}", x2: "{x}", y2: "{tip_y + tip_h}", stroke: "#10b981", stroke_width: "1" }
                                    rect { x: "{tip_x}", y: "{tip_y}", width: "{tip_w}", height: "{tip_h}", rx: "6", fill: "#0f172a", stroke: "#334155", stroke_width: "1" }
                                    text { x: "{tip_x + 8.0}", y: "{tip_y + 16.0}", class: "fill-current text-[11px] text-slate-300", "{date_label}" }
                                    text { x: "{tip_x + 8.0}", y: "{tip_y + 30.0}", class: "fill-current text-[11px] text-slate-200", "{value_label}" }
                                }}
                            }
                            None => rsx!{ Fragment {} }
                        }
                    }
                    {
                        points.iter().enumerate().scan(HashSet::<String>::new(), |printed, (i, p)| {
                            if p.date.len() >= 7 {
                                let m = &p.date[..7];
                                if printed.insert(m.to_string()) {
                                    let x = padding + (i as f32) * (6.0 + bar_gap);
                                    let node = rsx!{ text { x: "{x}", y: "{height + padding + 14.0}", class: "text-slate-400 fill-current text-[10px]", "{m}" } };
                                    return Some(Some(node));
                                }
                            }
                            Some(None)
                        }).filter_map(|x| x)
                    }
                }
            }
        }
    }
}
