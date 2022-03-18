use anyhow::{bail, Result};
use clap::Parser;
use flate2::{read::ZlibDecoder, write::ZlibEncoder, Compression};
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::Digest;
use simple_logger::SimpleLogger;
use std::{
    collections::HashMap,
    fs::File,
    hash::{Hash, Hasher},
    io::{Read, Write},
    os::raw::c_char,
    path::{Path, PathBuf},
    sync::Mutex,
};
use walkdir::WalkDir;

static DB_FILENAME: &str = "modland_hash.db";
static DB_REMOTE: &str = "https://www.dropbox.com/s/o5z6ffnyl7zzoo0/modland_hash.db?dl=1";
static DB_VERSION: u32 = 0x0000_00_01; // Version of database that has to match. (0.0.1)
static DB_SIZE: usize = 700_0000;

#[repr(C)]
struct CData {
    hash: u64,
    sample_names: *const c_char,
    artist: *const c_char,
    comments: *const c_char,
    channel_count: i32,
}

extern "C" {
    fn hash_file(data: *const u8, len: u32, dump_patterns: i32) -> *const CData;
    fn free_hash_data(data: *const CData);
}

fn get_string_cstr(c: *const c_char) -> String {
    unsafe { std::ffi::CStr::from_ptr(c).to_string_lossy().into_owned() }
}

#[derive(Clone, Default)]
struct TrackInfo {
    pattern_hash: u64,
    sha256_hash: String,
    filename: String,
    sample_names: String,
}

#[derive(Default, Debug, Serialize, Deserialize)]
struct DatabaseMeta {
    filename: String,
    samples: String,
}

impl PartialEq for DatabaseMeta {
    fn eq(&self, other: &Self) -> bool {
        self.filename == other.filename
    }
}

impl Hash for DatabaseMeta {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.filename.hash(state);
    }
}

impl Eq for DatabaseMeta {}

#[derive(Default, Serialize, Deserialize)]
struct Database {
    metadata: Vec<DatabaseMeta>,
    sha_hash: HashMap<String, Vec<usize>>,
    pattern_hash: HashMap<u64, Vec<usize>>,
}

impl Database {
    fn new(size: usize) -> Database {
        Database {
            metadata: Vec::with_capacity(size),
            sha_hash: HashMap::with_capacity(size),
            pattern_hash: HashMap::with_capacity(size),
        }
    }
}

/// Modland hashing
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Builds a new database given a local directory
    #[clap(short, long)]
    build_database: Option<String>,

    /// Download the remote database (done automaticlly if it doesn't exist)
    #[clap(short, long)]
    download_database: bool,

    /// Directory to search against the database. If not specificed the current directory will be used.
    #[clap(short, long, default_value = ".")]
    match_dir: String,

    /// Do recurseive scanning (include sub-directories) when using --match-dir and --build-database
    #[clap(short, long)]
    recursive: bool,

    /// If any duplicates includes these file extensions, they will be skipped. Example: --skip-file-extensions "mdx,pdx" will skip all dupes containing .mdx and .pdx files (ignores case)
    #[clap(long, default_value = "")]
    exclude_file_extensions: String,

    /// If any duplicate match these paths, they will be skipped. Example: --exclude-paths "/pub/favourites" will only show show results where "/pub/favourites" isn't present.
    #[clap(long, default_value = "")]
    exclude_paths: String,

    /// Only include If any duplicates includes these file extensions, other files will be skipped. Example: --include-file-extensions "mod,xm" will only show matches for .mod and .xm files
    #[clap(short, long, default_value = "")]
    include_file_extensions: String,

    /// Only include match if any duplicates maches these/this file path(s). Example: --include-paths "/incoming" will only show results when at least one file matches "/incoming"
    #[clap(long, default_value = "")]
    include_paths: String,

    /// Only include match if one of the duplicates matches the regexp pattern. Example: --include_sample_name ".*ripped.*" will only show duplicates where one of the tracks includes sample name(s) include "ripped"
    #[clap(long, default_value = "")]
    include_sample_name: String,

    /// Makes it possible to print sample names if pattern hash mismatches
    #[clap(short, long)]
    print_samples_pattern_hash: bool,

    /// List existing duplicates in the database
    #[clap(short, long)]
    list_duplicateds_in_database: bool,

    /// Mostly a debug option to allow dumping pattern data when both building database and matching entries
    #[clap(long)]
    dump_patterns: bool,

    /// Searches the whole database for a sample name with a regexp. The result will include all matching files, not just duplicates. --Example: --search-db--sample-name ".*BBS.*" matches all files that includes "BBS" in one of the samples (case-insensitive)
    #[clap(long, default_value = "")]
    search_db_sample_name: String,
}

struct Filters {
    include_paths: Vec<String>,
    include_file_extensions: Vec<String>,
    exclude_paths: Vec<String>,
    exclude_file_extensions: Vec<String>,
    sample_search: Option<Regex>,
}

impl Filters {
    fn init_filter(filter: &str, prefix: &str) -> Vec<String> {
        if filter.is_empty() {
            return Vec::new();
        }

        let mut output = Vec::new();

        for t in filter.split(",") {
            output.push(format!("{}{}", prefix, t));
        }

        output
    }

    fn new(args: &Args) -> Filters {
        let sample_search = if !args.include_sample_name.is_empty() {
            Some(Regex::new(&args.include_sample_name).unwrap())
        } else {
            None
        };

        Filters {
            include_paths: Self::init_filter(&args.include_paths, ""),
            include_file_extensions: Self::init_filter(&args.include_file_extensions, "."),
            exclude_paths: Self::init_filter(&args.exclude_paths, ""),
            exclude_file_extensions: Self::init_filter(&args.exclude_file_extensions, "."),
            sample_search,
        }
    }

    fn starts_with(filename: &str, tests: &[String], default_val: bool) -> bool {
        if tests.is_empty() {
            default_val
        } else {
            tests.iter().any(|t| filename.starts_with(t))
        }
    }

    fn ends_with(filename: &str, tests: &[String], default_val: bool) -> bool {
        if tests.is_empty() {
            default_val
        } else {
            tests.iter().any(|t| filename.ends_with(t))
        }
    }

    // Apply all the filters
    fn apply_filter<'a>(
        &self,
        input: &[&'a DatabaseMeta],
        skip_level: usize,
    ) -> Vec<&'a DatabaseMeta> {
        let mut output = Vec::new();

        for i in input {
            let filename = &i.filename;

            if !Self::starts_with(filename, &self.exclude_paths, false)
                && !Self::ends_with(filename, &self.exclude_file_extensions, false)
            {
                if Self::starts_with(filename, &self.include_paths, true)
                    && Self::ends_with(filename, &self.include_file_extensions, true)
                {
                    output.push(*i);
                }
            }
        }

        if let Some(re) = self.sample_search.as_ref() {
            for file in &output {
                if !file.samples.is_empty() {
                    if re.is_match(&file.samples.to_ascii_lowercase()) {
                        if output.len() >= skip_level {
                            return output;
                        } else {
                            return Vec::new();
                        }
                    }
                }
            }

            return Vec::new();
        }

        if output.len() >= skip_level {
            output
        } else {
            Vec::new()
        }
    }
}

// Get files for a given directory
fn get_files(path: &str, recurse: bool) -> Vec<String> {
    if !Path::new(path).exists() {
        println!(
            "Path/File \"{}\" doesn't exist. No file(s) will be processed.",
            path
        );
        return Vec::new();
    }

    // Check if "path" is a single file
    let md = std::fs::metadata(path).unwrap();

    if md.is_file() {
        return vec![path.to_owned()];
    }

    let spinner_style = ProgressStyle::with_template("{prefix:.bold.dim} {spinner} {wide_msg}")
        .unwrap()
        .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ ");

    let pb = ProgressBar::new(0);
    pb.set_style(spinner_style);
    pb.set_prefix(format!("Fetching list of files... [{}/?]", 0));

    let max_depth = if !recurse { 1 } else { usize::MAX };

    let files: Vec<String> = WalkDir::new(path)
        .max_depth(max_depth)
        .into_iter()
        .filter_map(|e| {
            let file = e.unwrap();
            let metadata = file.metadata().unwrap();

            if let Some(filename) = file.path().to_str() {
                if metadata.is_file() && !filename.ends_with(".listing") {
                    pb.set_message(filename.to_owned());
                    return Some(filename.to_owned());
                }
            }
            None
        })
        .collect();
    files
}

fn get_url(filename: &str) -> String {
    let url = filename.replace(" ", "%20");
    format!("https://ftp.modland.com{}", url)
}

// Fetches info for a track/song
fn get_track_info(filename: &str, dump_patterns: bool) -> TrackInfo {
    // Calculate sha256 of the file
    let mut file = File::open(&filename).unwrap();
    let mut file_data = Vec::new();
    file.read_to_end(&mut file_data).unwrap();
    let hash = sha2::Sha256::digest(&file_data);
    let dump_patterns = if dump_patterns { 1 } else { 0 };

    let song_data = unsafe { hash_file(file_data.as_ptr(), file_data.len() as _, dump_patterns) };

    let mut track_info = TrackInfo {
        filename: filename.to_owned(),
        sha256_hash: format!("{:x}", hash),
        ..Default::default()
    };

    if !song_data.is_null() {
        let hash_id = unsafe { (*song_data).hash };
        let sample_names = unsafe { get_string_cstr((*song_data).sample_names) };
        track_info.sample_names = sample_names;
        track_info.pattern_hash = hash_id;

        unsafe { free_hash_data(song_data) };
    }

    track_info
}

// Check that database version is valid
fn is_valid_db_version<P: AsRef<Path>>(path: P) -> Result<bool> {
    let mut version: [u8; 4] = [0; 4];

    log::trace!("Loading Database... [Reading]");

    let mut file = File::open(path)?;
    file.read(&mut version)?;

    let v = ((version[0] as u32) << 24)
        | ((version[1] as u32) << 16)
        | ((version[2] as u32) << 8)
        | ((version[3] as u32) << 0);

    if v == DB_VERSION {
        Ok(true)
    } else {
        Ok(false)
    }
}

// Get the target filename
fn get_db_filename() -> String {
    let p = std::env::current_exe().unwrap();
    let path = Path::new(&p);
    let path = path.parent().unwrap().join(DB_FILENAME);
    path.into_os_string().into_string().unwrap()
}

// Updates the database with new entries
fn build_database(out_filename: &str, database_path: &str, args: &Args) {
    let files = get_files(database_path, args.recursive);

    let spinner_style =
        ProgressStyle::with_template("{prefix:.bold.dim} {wide_bar} {pos}/{len}").unwrap();

    let pb = ProgressBar::new(files.len() as _);
    pb.set_style(spinner_style);

    //let database = Mutex::new(Database::new(DB_SIZE));
    let tracks_mt = Mutex::new(Vec::with_capacity(DB_SIZE));
    pb.set_prefix("Hashing files");

    files.par_iter().for_each(|input_path| {
        let mut track = get_track_info(input_path, args.dump_patterns);
        track.filename = input_path.replace(database_path, "");

        {
            let mut tracks = tracks_mt.lock().unwrap();
            tracks.push(track);
        }

        pb.inc(1);
    });

    println!("Writing database to disk... [Encoding]");

    let tracks = tracks_mt.lock().unwrap();
    let mut db = Database::new(DB_SIZE);

    for (index, track) in tracks.iter().enumerate() {
        if let Some(t) = db.sha_hash.get_mut(&track.sha256_hash) {
            t.push(index);
        } else {
            db.sha_hash
                .insert(track.sha256_hash.to_owned(), vec![index]);
        }

        if track.pattern_hash != 0 {
            if let Some(t) = db.pattern_hash.get_mut(&track.pattern_hash) {
                t.push(index);
            } else {
                db.pattern_hash.insert(track.pattern_hash, vec![index]);
            }

            db.metadata.push(DatabaseMeta {
                filename: track.filename.to_owned(),
                samples: track.sample_names.to_owned(),
            });
        } else {
            db.metadata.push(DatabaseMeta {
                filename: track.filename.to_owned(),
                samples: String::new(),
            });
        }
    }

    //let db = database.lock().unwrap();
    let encoded: Vec<u8> = bincode::serialize(&db).unwrap();

    println!("Writing database to disk... [Compressing]");

    let mut e = ZlibEncoder::new(Vec::new(), Compression::best());
    e.write_all(&encoded).unwrap();
    let compressed_bytes = e.finish().unwrap();

    println!("Writing database to disk... [Writing]");

    let database_version = [
        (DB_VERSION >> 24) as u8,
        (DB_VERSION >> 16) as u8,
        (DB_VERSION >> 8) as u8,
        (DB_VERSION >> 0) as u8,
    ];

    let mut file = File::create(out_filename).unwrap();
    file.write_all(&database_version).unwrap();
    file.write_all(&compressed_bytes).unwrap();

    println!("Writing database to disk... [Done]");
}

fn create_db_file() -> Result<File> {
    let filename = get_db_filename();

    if let Ok(file) = File::create(&filename) {
        return Ok(file);
    }

    bail!(
        "Tried to create database at {} but was unable to do so. Manually download {} and place it next to the modland_has executable",
        filename, DB_REMOTE,
    )
}

// Download the remote file
fn download_db_err() -> Result<()> {
    let mut file = create_db_file()?;

    let resp = ureq::get(DB_REMOTE).call()?;
    let len: usize = resp.header("Content-Length").unwrap().parse()?;

    let mut temp_buffer: [u8; 1024] = [0; 1024];
    let mut reader = resp.into_reader();

    let pb = ProgressBar::new(len as _);
    pb.set_style(
        ProgressStyle::with_template(
            "{prefix:.bold.dim} {spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes}",
        )
        .unwrap()
        .with_key("eta", |state| format!("{:.1}s", state.eta().as_secs_f64()))
        .progress_chars("#>-"),
    );

    pb.set_prefix("Downloading Database");

    let mut pos = 0;

    loop {
        let read_size = reader.read(&mut temp_buffer)?;

        if read_size == 0 {
            break;
        }

        pb.set_position(pos);
        pos += read_size as u64;

        file.write_all(&temp_buffer[0..read_size])?;
    }

    Ok(())
}

fn download_db() {
    match download_db_err() {
        Err(_) => {
            println!("Unable to download database. Download {} manually and place it next to the executable.", DB_REMOTE);
            std::process::exit(1);
        }
        _ => (),
    }
}

fn load_database(filename: &str) -> Result<Database> {
    let mut data = Vec::new();
    let mut decompressed_data = Vec::new();
    let mut version: [u8; 4] = [0, 0, 0, 0];

    log::trace!("Loading Database... {} [Reading]", filename);

    let mut file = std::fs::File::open(filename)?;
    file.read(&mut version)?;

    // TODO: check version
    file.read_to_end(&mut data)?;

    log::trace!("Loading Database... [Decompressing]");

    let mut z = ZlibDecoder::new(&data[..]);
    z.read_to_end(&mut decompressed_data)?;

    log::trace!("Loading Database... [Unencoding]");

    let datbase: Database = bincode::deserialize(&decompressed_data[..])?;

    Ok(datbase)
}

/*
    let re = Regex::new(search_string).unwrap();
    let mut count = 0;

    tracks.iter().for_each(|track| {
        if let Some(metadata) = track.metadata.as_ref() {
            if re.is_match(&metadata.sample_names.to_ascii_lowercase()) {
                println!("===============================================================");
                println!("Matching {}", track.filename);
                println!("{}", metadata.sample_names);
                count += 1;
            }
        }
    });

    println!("Total matches {}", count);
}
     */

fn get_files_from_sha_hash<'a>(info: &TrackInfo, lookup: &'a Database) -> Vec<&'a DatabaseMeta> {
    let mut entries = Vec::new();

    if let Some(res) = lookup.sha_hash.get(&info.sha256_hash) {
        for v in res {
            entries.push(&lookup.metadata[*v]);
        }
    }

    entries
}

fn get_files_from_pattern_hash<'a>(
    info: &TrackInfo,
    lookup: &'a Database,
) -> Vec<&'a DatabaseMeta> {
    let mut entries = Vec::new();
    if let Some(res) = lookup.pattern_hash.get(&info.pattern_hash) {
        for v in res {
            entries.push(&lookup.metadata[*v]);
        }
    }

    entries
}

fn print_samples_with_outline(samples: &str) {
    // figure out the max len of the lines
    let mut last_line_with_text = 0;
    let mut max_len = 0;
    for (index, line) in samples.lines().enumerate() {
        max_len = std::cmp::max(line.len(), max_len);
        if !line.is_empty() {
            last_line_with_text = index;
        }
    }

    // spacing on each side
    max_len += 2;

    print!("┌");

    for _in in 0..max_len {
        print!("─");
    }

    println!("┐");

    for (index, line) in samples.lines().enumerate() {
        print!("│ ");
        print!("{}", line);

        for _ in line.len()..max_len - 1 {
            print!(" ");
        }

        println!("│");

        if index == last_line_with_text {
            break;
        }
    }

    print!("└");
    for _in in 0..max_len {
        print!("─");
    }

    println!("┘");
}

fn match_dir_against_db(dir: &str, args: &Args, lookup: &Database) {
    let files = get_files(dir, args.recursive);

    let filters = Filters::new(args);

    //files.par_iter().for_each(|filename| {
    files.iter().for_each(|filename| {
        let info = get_track_info(filename, args.dump_patterns);
        let mut output = String::new();

        println!("Matching {}", filename);

        let filenames = get_files_from_sha_hash(&info, lookup);
        let filenames_pattern = get_files_from_pattern_hash(&info, lookup);

        let filenames = filters.apply_filter(&filenames, 1);
        let filenames_pattern = filters.apply_filter(&filenames_pattern, 1);

        let mut found_entries = HashMap::new();

        for entry in &filenames {
            found_entries.insert(*entry, (true, false));
        }

        for entry in &filenames_pattern {
            if let Some(v) = found_entries.get_mut(*entry) {
                v.1 = true;
            } else {
                found_entries.insert(entry, (false, true));
            }
        }

        let mut printed_initial_samples = false;

        for found in &found_entries {
            let url = get_url(&found.0.filename);
            if found.1 .0 && found.1 .1 {
                println!("Found match {} (hash) (pattern_hash)", url);
            } else if found.1 .0 && !found.1 .1 {
                println!("Found match {} (hash)", url);
            } else if args.print_samples_pattern_hash {
                if !printed_initial_samples && args.print_samples_pattern_hash {
                    print_samples_with_outline(&info.sample_names);
                    printed_initial_samples = true;
                }
                println!("Found match {} (pattern_hash)", url);
                print_samples_with_outline(&found.0.samples);
            } else {
                println!("Found match {} (pattern_hash)", url);
            }
        }

        if found_entries.is_empty() {
            println!("No matches found!");
        }

        println!();

        output.push_str(&format!("Matching {}", filename));
    });
}

// First check if we have a database next to the to the exe, otherwise try local directory
fn check_for_db_file() -> Option<PathBuf> {
    let path = Path::new(&get_db_filename()).to_path_buf();
    if path.exists() {
        return Some(path);
    } else {
    }

    None
}

fn print_db_duplicates(db: &Database, args: &Args) {
    let mut hash_dupes = Vec::with_capacity(700_0000);
    let metadata = &db.metadata;
    let filters = Filters::new(args);

    for (_key, val) in db.sha_hash.iter() {
        if val.len() <= 1 {
            continue;
        }

        let mut vals = Vec::with_capacity(val.len());

        for v in val {
            vals.push(&metadata[*v]);
        }

        let mut vals = filters.apply_filter(&vals, 2);

        if !vals.is_empty() {
            // sort the individual entries
            vals.sort_by(|a, b| a.filename.cmp(&b.filename));
            hash_dupes.push(vals);
        }
    }

    if hash_dupes.is_empty() {
        return;
    }

    // sort the whole array to have deterministic output
    hash_dupes.sort_by(|a, b| a[0].filename.cmp(&b[0].filename));

    for (index, v) in hash_dupes.iter().enumerate() {
        println!("\n==================================================================");
        println!("Dupe Entry {}", index);

        for e in v {
            println!("{}", get_url(&e.filename));

            if filters.sample_search.is_some() {
                print_samples_with_outline(&e.samples);
            }
        }
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    SimpleLogger::new()
        .with_level(log::LevelFilter::Off)
        .init()?;

    // first we check if we have a database and if we don't we try to download it we don't
    // or if the database version doesn't match

    if let Some(db_path) = args.build_database.as_ref() {
        let filename = get_db_filename();

        if std::path::Path::new(&filename).exists() {
            std::fs::remove_file(&filename).unwrap();
        }

        build_database(&filename, db_path, &args);

        return Ok(());
    }

    let database_path = check_for_db_file();

    if args.download_database || database_path.is_none() {
        download_db();
    }

    if let Some(path) = database_path {
        if !is_valid_db_version(path)? {
            println!("Database version doesn't match executable. Downloading");
            download_db();
        }
    }

    println!("Loading database...");

    let database = load_database(&get_db_filename()).unwrap();

    println!("Loading database... [Done]\n");

    // Process duplicates in the database
    if args.list_duplicateds_in_database {
        print_db_duplicates(&database, &args);
        return Ok(());
    }

    match_dir_against_db(&args.match_dir, &args, &database);

    Ok(())
}

/*
let mut output = String::with_capacity(10 * 1024 * 1024);
let mut count = 0;
let map = data.lock().unwrap();

//output.push_str(HTML_HEADER);

let mut dupe_array = Vec::new();

for (_key, val) in map.iter() {
    if val.len() > 1 {
        dupe_array.push(val);
    }
}open_in_memory

        if t.filename.contains("pub/favourites") {
            found_unknown = false;
            break;
        }
    }

    if found_unknown {
        output.push_str(&format!("Dupe {}\n", count));
        output.push_str("----------------------\n\n");

        for t in val {
            let name = &t.filename[18..];
            let url_name = name.replace(" ", "%20");
            output.push_str(&format!("[{}](https://{})\n", name, url_name));
            output.push_str("```\n");
            output.push_str(&t.sample_names.trim_end_matches('\n'));
            output.push_str("\n```\n");
        }
        count += 1;
    }
}
*/

/*
static HTML_HEADER: &str =
    "<html xmlns=\"http://www.w3.org/1999/xhtml\" xml:lang=\"en\" lang=\"en\">
<head>
    <style type=\"text/css\" media=\"screen\">
        body {Write;
            border: 1px solid #999;
            display: block;
            padding: 20px;
        }
    </style>
</head>

";
 */
