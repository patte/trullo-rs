use dioxus::prelude::*;

#[allow(non_snake_case)]
#[component]
pub fn Gauge(
    value: i32,
    start_angle: f32,
    stop_angle: f32,
    size: i32,
    stroke: i32,
    track_class: String,
    progress_class: String,
    children: Element,
) -> Element {
    // Normalize & clamp
    let val = value.clamp(0, 100) as f32;
    let span = (stop_angle - start_angle).abs().max(0.0001);
    let end_angle = start_angle + span * (val / 100.0);

    let c = (size as f32) / 2.0;
    let r = c - (stroke as f32) / 2.0 - 1.0; // small padding

    // Helpers to make arc paths
    fn to_rad(deg: f32) -> f32 {
        deg.to_radians()
    }
    fn polar(cx: f32, cy: f32, r: f32, ang: f32) -> (f32, f32) {
        let rad = to_rad(ang);
        (cx + r * rad.cos(), cy + r * rad.sin())
    }
    fn arc_path(cx: f32, cy: f32, r: f32, a0: f32, a1: f32) -> String {
        let (x0, y0) = polar(cx, cy, r, a0);
        let (x1, y1) = polar(cx, cy, r, a1);
        let delta = (a1 - a0).abs();
        let large_arc = if delta >= 180.0 { 1 } else { 0 };
        let sweep = if a1 >= a0 { 1 } else { 0 };
        format!("M {x0:.3} {y0:.3} A {r:.3} {r:.3} 0 {large_arc} {sweep} {x1:.3} {y1:.3}")
    }

    // Rotate gauge 90 degrees clockwise for more natural orientation
    let angle_offset = 90.0;
    let start0 = start_angle + angle_offset;
    let stop0 = stop_angle + angle_offset;
    let end0 = end_angle + angle_offset;

    let track_d = arc_path(c, c, r, start0, stop0);
    let progress_d = arc_path(c, c, r, start0, end0);

    let size_attr = size.to_string();
    let view_box = format!("0 0 {size} {size}");
    let stroke_width = stroke.to_string();
    let container_style = format!("width:{size}px;height:{size}px");

    rsx! {
        div { class: "relative", style: "{container_style}",
            svg { width: "{size_attr}", height: "{size_attr}", view_box: "{view_box}",
                // Track
                path { class: "{track_class}", d: "{track_d}", fill: "none", stroke: "currentColor", stroke_width: "{stroke_width}", stroke_linecap: "round" }
                // Progress
                path { class: "{progress_class}", d: "{progress_d}", fill: "none", stroke: "currentColor", stroke_width: "{stroke_width}", stroke_linecap: "round" }
            }
            // Center content
            div { class: "absolute inset-0 grid place-items-center", {children} }
        }
    }
}
