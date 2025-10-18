#[cfg(all(feature = "web"))]
pub fn pad2(n: i32) -> String {
    if n < 10 {
        format!("0{}", n)
    } else {
        n.to_string()
    }
}

#[cfg(all(feature = "web"))]
pub fn format_local(rfc3339: &str) -> String {
    use js_sys::Date;
    let d = Date::new(&wasm_bindgen::JsValue::from_str(rfc3339));
    if d.get_time().is_nan() {
        return rfc3339.to_string();
    }
    let day = d.get_date() as i32;
    let month = (d.get_month() as i32) + 1;
    let year = d.get_full_year() as i32;
    let hour = d.get_hours() as i32;
    let minute = d.get_minutes() as i32;
    format!(
        "{}.{}.{} {}:{}",
        pad2(day),
        pad2(month),
        year,
        pad2(hour),
        pad2(minute)
    )
}

#[cfg(not(all(feature = "web")))]
pub fn format_local(rfc3339: &str) -> String {
    rfc3339.to_string()
}

pub fn format_megabytes(mb: i32) -> String {
    if mb.abs() >= 1_000 {
        let gb = mb as f64 / 1_000.0;
        if (gb.fract()).abs() < f64::EPSILON || (gb * 10.0).round() % 10.0 == 0.0 {
            format!("{} GB", gb.round() as i32)
        } else {
            format!("{:.1} GB", gb)
        }
    } else {
        format!("{} MB", mb)
    }
}

pub fn format_megabytes_f32(mb: f32) -> String {
    if mb.abs() >= 1_000.0 {
        let gb = mb as f64 / 1_000.0;
        if (gb.fract()).abs() < f64::EPSILON || (gb * 10.0).round() % 10.0 == 0.0 {
            format!("{} GB", gb.round() as i32)
        } else {
            format!("{:.1} GB", gb)
        }
    } else {
        format!("{} MB", mb.round() as i32)
    }
}
