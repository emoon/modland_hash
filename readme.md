## About

modland_hash is a tool for [ftp.modland.com](https://ftp.modland.com) It's used to find duplicates and includes various filtering options.
The most basic use case is to run the tool and it will match the local files against the modland database and see if the files already exists on modland.
Commandline options are as following:
```
   -b, --build-database <BUILD_DATABASE>
            Builds a new database given a local directory

    -d, --download-database
            Download the remote database (done automaticlly if it doesn't exist)

        --dump-patterns
            Mostly a debug option to allow dumping pattern data when both building database and
            matching entries

        --exclude-file-extensions <EXCLUDE_FILE_EXTENSIONS>
            If any duplicates includes these file extensions, they will be skipped. Example: --skip-
            file-extensions "mdx,pdx" will skip all dupes containing .mdx and .pdx files (ignores
            case) [default: ]

        --exclude-paths <EXCLUDE_PATHS>
            If any duplicate match these paths, they will be skipped. Example: --exclude-paths
            "/pub/favourites" will only show show results where "/pub/favourites" isn't present
            [default: ]

    -h, --help
            Print help information

    -i, --include-file-extensions <INCLUDE_FILE_EXTENSIONS>
            Only include If any duplicates includes these file extensions, other files will be
            skipped. Example: --include-file-extensions "mod,xm" will only show matches for .mod and
            .xm files [default: ]

        --include-paths <INCLUDE_PATHS>
            Only include match if any duplicates maches these/this file path(s). Example: --include-
            paths "/incoming" will only show results when at least one file matches "/incoming"
            [default: ]

        --include-sample-name <INCLUDE_SAMPLE_NAME>
            Only include match if one of the duplicates matches the regexp pattern. Example:
            --include_sample_name ".*ripped.*" will only show duplicates where one of the tracks
            includes sample name(s) include "ripped" [default: ]

    -l, --list-duplicateds-in-database
            List existing duplicates in the database

    -m, --match-dir <MATCH_DIR>
            Directory to search against the database. If not specificed the current directory will
            be used [default: .]

    -p, --print-sample-names
            Makes it possible to print sample names

    -r, --recursive
            Do recurseive scanning (include sub-directories) when using --match-dir and --build-
            database

        --search-filename <SEARCH_FILENAME>
            Only display duplicate results if one of the hits include the maching filename. Example
            --search-filename ".*north.*" will only include dupe resutls if one of the entries has
            .*north.* in it (case-insensitive) [default: ]

    -V, --version
            Print version information
```

## Downloading

Builds of the tool can be found here https://github.com/emoon/modland_hash/releases (currently Windows only)

## Compiling

To compile the code you need to have the [Rust](https://www.rust-lang.org) compiler instead. You also need a C++ compiler which depends on your OS (usually MSVC on Windows and clang/gcc on *Nix, macOS)

1. Download Rust from here https://rustup.rs and follow the instructions for your platform
2. `git clone https://github.com/emoon/modland_hash`
3. `cd modland_hash && cargo build --release`
4. Use `modland_hash` by running `target/release/modland_hash` or `cargo run --release -- <command line ops here>`
