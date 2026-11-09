fn main() {
    glib_build_tools::compile_resources(
        &["src/assets", "setup/metainfo"],
        "src/assets/mimick.gresource.xml",
        "mimick.gresource",
    );
}
