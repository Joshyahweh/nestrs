fn main() {
    // Only generate gRPC code when the feature is enabled.
    if std::env::var("CARGO_FEATURE_GRPC").is_err() {
        return;
    }

    println!("cargo:rerun-if-changed=proto/nestrs_microservice.proto");

    if std::env::var("PROTOC").is_err() {
        let path = protoc_bin_vendored::protoc_bin_path()
            .expect("vendored protoc should be available");
        std::env::set_var("PROTOC", path);
    }

    tonic_prost_build::compile_protos("proto/nestrs_microservice.proto")
        .expect("tonic-prost-build should compile protos");
}

