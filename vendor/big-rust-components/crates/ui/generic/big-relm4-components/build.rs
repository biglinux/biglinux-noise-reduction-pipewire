fn main() {
    let resources = "resources";
    glib_build_tools::compile_resources(
        &[resources],
        "resources/big-components.gresource.xml",
        "big-components.gresource",
    );
    println!("cargo:rerun-if-changed={resources}");
}
