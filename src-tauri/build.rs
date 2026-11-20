fn main() {
    println!("cargo:rerun-if-changed=resources/app2api-runtime");
    tauri_build::build();
}
