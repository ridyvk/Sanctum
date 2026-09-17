fn main() {
    let release = std::env::var("PROFILE").is_ok_and(|profile| profile == "release");
    let tauri_dev = std::env::var("DEP_TAURI_DEV").is_ok_and(|value| value == "true");
    if release && tauri_dev {
        panic!(
            "refusing to build a release that points at the development server; \
             use the Tauri CLI production build so the custom-protocol feature is enabled"
        );
    }
    tauri_build::build()
}
