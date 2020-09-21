use std::env;
fn main() {
    if env::var("CARGO_CFG_WINDOWS").is_ok() {
        println!(r"cargo:rustc-link-search=C:\gstreamer\1.0\msvc_x86_64\lib")
    }
}
