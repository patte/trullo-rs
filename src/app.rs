use dioxus::prelude::*;

use crate::components::{DataStatusCard, UsageChartView};
use crate::{FAVICON, TAILWIND_CSS};

#[allow(non_snake_case)]
#[component]
pub fn App() -> Element {
    rsx! {
        document::Link { rel: "icon", href: FAVICON }
        document::Stylesheet { href: TAILWIND_CSS }
        document::Meta { name: "theme-color", content: "#020618" } // slate-950
        document::Meta { name: "color-scheme", content: "dark" }
        // Page container
        div { class: "min-h-screen bg-slate-950 text-slate-100 p-6 space-y-6",
            // Centered card (max-w-xl)
            div { class: "w-full max-w-xl mx-auto",
                DataStatusCard {}
            }
            // Full-width chart section
            div { class: "w-full max-w-5xl mx-auto",
                UsageChartView {}
            }
        }
    }
}
