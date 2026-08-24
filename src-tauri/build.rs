use std::path::Path;

fn main() {
    // The sidecars are not versioned, so a fresh clone hits this. Tauri's own
    // error names the missing path but not the way out of it.
    let triple = std::env::var("TARGET").unwrap_or_default();
    for tool in ["ffmpeg", "ffprobe"] {
        let path = format!("binaries/{tool}-{triple}");
        if !Path::new(&path).exists() {
            panic!(
                "\n\n  Missing sidecar: {path}\n\n  \
                 ffmpeg and ffprobe are not kept in the repository.\n  \
                 Fetch them first:\n\n      ./scripts/fetch-ffmpeg.sh\n\n  \
                 See THIRD-PARTY.md for why they are not shipped.\n"
            );
        }
    }

    tauri_build::build()
}
