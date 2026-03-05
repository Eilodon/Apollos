fn main() {
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("failed to resolve vendored protoc");
    std::env::set_var("PROTOC", protoc);

    println!("cargo:rerun-if-changed=proto/types.proto");
    println!("cargo:rerun-if-changed=proto/messages.proto");

    let mut config = prost_build::Config::new();
    config.compile_well_known_types();
    config.type_attribute(".", "#[derive(serde::Serialize, serde::Deserialize)]");

    config
        .compile_protos(&["proto/types.proto", "proto/messages.proto"], &["proto"])
        .expect("failed to compile protobuf contracts");
}
