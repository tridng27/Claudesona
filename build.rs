// Compiles her bundled voice (assets/voice/<hash>.mp3, made by `claudesona voices <voice_id>`) into the exe.
// No files there just means no bundled voice.
fn main() {
    println!("cargo:rerun-if-changed=assets/voice");
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join("voice");
    let mut files: Vec<_> = std::fs::read_dir(&dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "mp3")).collect();
    files.sort();
    let mut out = String::from("&[\n");
    for f in files {
        let name = f.file_stem().unwrap().to_string_lossy();
        out.push_str(&format!("    ({name:?}, include_bytes!({:?}) as &[u8]),\n", f.display().to_string()));
    }
    out.push(']');
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    std::fs::write(out_dir.join("voice.rs"), out).unwrap();
    // Whose voice it is, so chat and replies can be spoken live in the same voice.
    let id = std::fs::read_to_string(dir.join("voice_id.txt")).unwrap_or_default();
    std::fs::write(out_dir.join("voice_id.rs"), format!("{:?}", id.trim())).unwrap();
}
