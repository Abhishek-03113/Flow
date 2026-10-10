use std::fs;
use std::path::Path;

fn main() {
    // `tauri::generate_context!` embeds the built frontend and fails if the
    // directory is missing. Keep `cargo build/clippy` working on a fresh clone
    // (before `npm run build` in app/ui) with a placeholder page.
    let dist = Path::new("../ui/dist");
    if !dist.exists() {
        fs::create_dir_all(dist).expect("create ui/dist placeholder");
        fs::write(
            dist.join("index.html"),
            "<!doctype html><title>Flow</title><p>Run <code>npm run build</code> in app/ui.</p>",
        )
        .expect("write placeholder index.html");
    }
    tauri_build::build();
}
