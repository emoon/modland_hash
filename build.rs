use walkdir::WalkDir;

fn get_files(path: &str) -> Vec<String> {
    let files: Vec<String> = WalkDir::new(path)
        .into_iter()
        .filter_map(|e| {
            let file = e.unwrap();
            let metadata = file.metadata().unwrap();

            if let Some(filename) = file.path().to_str() {
                if metadata.is_file() && !filename.ends_with(".h") {
                    return Some(file);
                }
            }

            None
        })
        .map(|entry| entry.into_path().to_str().unwrap().to_string())
        .collect();
    files
}

fn add_files(build: &mut cc::Build, path: &str) {
    let files = get_files(path);
    build.files(files);
}

fn main() {
    let mut build = cc::Build::new();
    let env = std::env::var("TARGET").unwrap();

    println!("cargo:rerun-if-changed=external/libopenmpt");

    build.include("external/libopenmpt");
    build.include("external/libopenmpt/common");

    if env.contains("windows") {
        //
    } else if env.contains("darwin") {
        //
    } else {
        build.flag("-std=c++17");
        build.cpp_link_stdlib("stdc++");
    }

    build.define("LIBOPENMPT_BUILD", None);

    add_files(&mut build, "external/libopenmpt/soundlib");
    add_files(&mut build, "external/libopenmpt/common");
    add_files(&mut build, "external/libopenmpt/sounddsp");

    build.file("external/libopenmpt/libopenmpt/libopenmpt_c.cpp");
    build.file("external/libopenmpt/libopenmpt/libopenmpt_cxx.cpp");
    build.file("external/libopenmpt/libopenmpt/libopenmpt_impl.cpp");
    build.file("external/libopenmpt/libopenmpt/libopenmpt_ext_impl.cpp");
    build.file("external/libopenmpt/interface.cpp");

    build.compile("cpp_code");

    // linker stuff
    if env.contains("windows") {
        // todo fixme
    } else if env.contains("darwin") {
        //
    } else {
        println!("cargo:rustc-link-lib=stdc++");
    }
}
