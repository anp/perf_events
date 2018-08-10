extern crate bindgen;

use std::env;
use std::path::PathBuf;

fn main() {
    let mut bindings = bindgen::Builder::default().header("wrapper.h");

    if std::env::var("TARGET").unwrap().find("linux").is_none() {
        bindings = bindings.clang_arg("-Ilinux-headers");
    }

    let generated = bindings.generate().expect("Unable to generate bindings");

    // Write the bindings to the $OUT_DIR/bindings.rs file.
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    generated
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");

    println!("cargo:rerun-if-changed=wrapper.h");
}
