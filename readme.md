## About

modland_hash is a tool for [ftp.modland.com](https://ftp.modland.com) It's used to find duplicates and includes various filtering options.
The most basic use case is to run the tool and it will match the local files against the modland database and see if the files already exists on modland.
Commandline options are as following:
```
  -b, --build-database <BUILD_DATABASE>
          Builds a new database from a given local directory
  -d, --download-database
          Downloads the remote database (automatically performed if it doesn't exist)
  -m, --match-dir <MATCH_DIR>
          Directory to search against the database. If not specified, the current directory will be used [default: .]
  -r, --recursive
          Performs recursive scanning (includes sub-directories) when using --match-dir and --build-database
      --match-samples
          Instead of matching on hash or pattern hash match the samples in the files
      --find-samples-with-length <FIND_SAMPLES_WITH_LENGTH>
          Search the database for samples matching a certain length (length is in samples)
      --find-samples-with-length-bytes <FIND_SAMPLES_WITH_LENGTH_BYTES>
          Search the database for samples matching a certain length (length is in bytes)
      --exclude-file-extensions <EXCLUDE_FILE_EXTENSIONS>
          Skips files with these extensions if any duplicates are found. Example: --skip-file-extensions "mdx,pdx" will skip all duplicates that contain .mdx and .pdx files (case-insensitive) [default: ]
      --exclude-paths <EXCLUDE_PATHS>
          Skips duplicates that match these paths. Example: --exclude-paths "/pub/favourites" will exclude results where "/pub/favourites" is present [default: ]
  -i, --include-file-extensions <INCLUDE_FILE_EXTENSIONS>
          Includes only duplicates with these file extensions; other files will be skipped. Example: --include-file-extensions "mod,xm" will include only matches for .mod and .xm files [default: ]
      --include-paths <INCLUDE_PATHS>
          Includes matches only if duplicates match these file paths. Example: --include-paths "/incoming" will show results only when at least one file matches "/incoming" [default: ]
      --include-sample-name <INCLUDE_SAMPLE_NAME>
          Includes matches only if one of the duplicates matches the specified regexp pattern for sample names. Example: --include_sample_name ".*ripped.*" will include duplicates where one of the tracks' sample names contains "ripped" [default: ]
      --search-filename <SEARCH_FILENAME>
          Displays duplicate results only if one of the entries includes a matching filename. Example: --search-filename ".*north.*" will include results only if one of the entries has "north" in it (case-insensitive) [default: ]
  -p, --print-sample-names
          Enables printing of sample names
  -l, --list-duplicates-in-database
          Lists existing duplicates in the database
      --list-database
          Dumps all information in the database
      --dump-patterns
          Primarily a debug option to allow dumping of pattern data when building the database and matching entries
  -h, --help
          Print help
  -V, --version
          Print version
  -b, --build-database <BUILD_DATABASE>
            Builds a new database given a local directory
```

## Downloading

Builds of the tool can be found here https://github.com/emoon/modland_hash/releases (Windows and macOS) 

## Examples

Find all samples in the database with a length of 8700 and matching the text "ahhvox"

```
modland_hash --find-samples-with-length-bytes 8700 --include-sample-name '.*ahhvox.*'`
```

To match the local files just run the tool without any options

```
modland_hash
```

To match the local files but only include .mod and .xm files 

```
modland_hash --include-file-extensions "mod,xm"
```

To match the local files but only include .mod and .xm files and only include duplicates that match the "/incoming" path

```
modland_hash --include-file-extensions "mod,xm" --include-paths "/incoming"
```

To match the local files but only include .mod and .xm files and only include duplicates that match the "/incoming" path and only include duplicates that match the ".*north.*" regexp pattern in the filename

```
modland_hash --include-file-extensions "mod,xm" --include-paths "/incoming" --search-filename ".*north.*"
```

Match the local files but match the samples in the files instead of the files themselves

```
modland_hash --match-samples
```

## Database

The database is a SQLite database that can be downloaded from here https://www.dropbox.com/scl/fi/gtk2yri6iizlaeb6b0j0j/modland_hash.db.7z?rlkey=axcrqv54eg2c1yju6vf043ly1&dl=1
It's updated every 24 hours and contains all the files on ftp.modland.com. If the modland_hash tool doesn't fit your needs, you can use the database directly in a tool such as https://sqliteonline.com or write your own tool to query the database. 

## Compiling

To compile the code you need to have the [Rust](https://www.rust-lang.org) compiler instead. You also need a C++ compiler which depends on your OS (usually MSVC on Windows and clang/gcc on *Nix, macOS)

1. Download Rust from here https://rustup.rs and follow the instructions for your platform
2. `git clone https://github.com/emoon/modland_hash`
3. `cd modland_hash && cargo build --release`
4. Use `modland_hash` by running `target/release/modland_hash` or `cargo run --release -- <command line ops here>`
