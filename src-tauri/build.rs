use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn find_vlc_dir() -> Option<PathBuf> {
    for candidate in [
        r"C:\Program Files\VideoLAN\VLC",
        r"C:\Program Files (x86)\VideoLAN\VLC",
    ] {
        let path = PathBuf::from(candidate);
        if path.join("libvlc.dll").exists() {
            return Some(path);
        }
    }
    env::var("VLC_DIR").ok().map(PathBuf::from)
}

fn main() {
    tauri_build::build();

    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        match find_vlc_dir() {
            Some(vlc_dir) => {
                // Copy the DLLs next to the built exe so Windows can find them at
                // runtime (VLC's install dir isn't on PATH by default).
                let out_dir = env::var("OUT_DIR").unwrap();
                let target_dir = Path::new(&out_dir)
                    .ancestors()
                    .nth(3)
                    .expect("unexpected OUT_DIR layout")
                    .to_path_buf();

                for dll in ["libvlc.dll", "libvlccore.dll"] {
                    let src = vlc_dir.join(dll);
                    if src.exists() {
                        let _ = fs::copy(&src, target_dir.join(dll));
                    }
                }

                println!(
                    "cargo:rustc-env=VLC_PLUGIN_DIR={}",
                    vlc_dir.join("plugins").display()
                );
            }
            None => {
                println!(
                    "cargo:warning=Could not locate a VLC installation; the app will fail to load libvlc.dll at runtime. Set the VLC_DIR env var to your VLC install path."
                );
            }
        }
    }
}
