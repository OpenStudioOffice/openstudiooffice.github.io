mod app;
mod engine;

// ── Desktop (cargo run --release) ──
#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    env_logger::init();
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 760.0])
            .with_title("Open Studio Office"),
        ..Default::default()
    };
    eframe::run_native(
        "Open Studio Office",
        options,
        Box::new(|cc| Ok(Box::new(app::SheetApp::new(cc)))),
    )
}

// ── Browser / GitHub Pages (trunk build) ──
#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wasm_bindgen::JsCast as _;

    eframe::WebLogger::init(log::LevelFilter::Info).ok();
    let web_options = eframe::WebOptions::default();

    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window()
            .expect("No window")
            .document()
            .expect("No document");
        let canvas = document
            .get_element_by_id("the_canvas_id")
            .expect("Failed to find the_canvas_id")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("the_canvas_id was not a HtmlCanvasElement");

        let result = eframe::WebRunner::new()
            .start(
                canvas,
                web_options,
                Box::new(|cc| Ok(Box::new(app::SheetApp::new(cc)))),
            )
            .await;

        if let Err(e) = result {
            log::error!("Failed to start Open Studio Office: {e:?}");
        }
    });
}
