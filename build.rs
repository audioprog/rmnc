fn main() {
    slint_build::compile_with_config(
        "ui/main.slint",
        slint_build::CompilerConfiguration::new(),
    )
    .expect("Slint build failed");
}
