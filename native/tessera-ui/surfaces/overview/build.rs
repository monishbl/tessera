// This crate's Slint (ui/main.slint and what it imports), with the shared
// files (theme, widgets, symbols) as the `@tessera` library: core/ui/; and
// Search's view (typing in the Overview searches) as `@spotlight`:
// surfaces/spotlight/ui/
fn main() {
    let shared = std::collections::HashMap::from([
        ("tessera".to_string(), std::path::PathBuf::from("../../core/ui")),
        ("spotlight".to_string(), std::path::PathBuf::from("../spotlight/ui")),
    ]);
    let config = slint_build::CompilerConfiguration::new().with_library_paths(shared);
    slint_build::compile_with_config("ui/main.slint", config).expect("ui/main.slint");
}
