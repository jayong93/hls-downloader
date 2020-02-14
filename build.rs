use std::env;
fn main() {
    if env::var("CARGO_CFG_WINDOWS").is_ok() {
        println!(r"cargo:rustc-link-search=C:\gstreamer-win\1.0\x86_64\lib")
    }
}
