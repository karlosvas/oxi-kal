fn main() {
    let config = slint_build::CompilerConfiguration::new()
        .with_library_paths(oxide_theme::slint_library());
    slint_build::compile_with_config("ui/app.slint", config).expect("error compilando la UI Slint");
    println!("cargo:rerun-if-changed=ui");
    println!("cargo:rerun-if-changed={}", oxide_theme::slint_dir().display());
}
