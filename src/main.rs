#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod doc;
mod docx;
mod editor;
mod fonts;
mod io;
mod layout;
mod numbering;
mod pdf;
mod pool;
mod styles;
mod ui;
mod xml;

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 4 && args[1] == "--convert" {
        if let Err(e) = app::convert(args[2].as_ref(), args[3].as_ref()) {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("FreeWord")
            .with_inner_size([1280.0, 860.0])
            .with_min_inner_size([640.0, 400.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native("FreeWord", options, Box::new(|cc| Ok(Box::new(app::App::new(cc)))))
}
