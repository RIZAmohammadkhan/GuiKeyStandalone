// build.rs
extern crate embed_manifest;

fn main() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        embed_manifest::embed_manifest(embed_manifest::new_manifest("App"))
            .expect("unable to embed manifest");
    }
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=App.manifest"); // Ensure rebuild if manifest changes
}
