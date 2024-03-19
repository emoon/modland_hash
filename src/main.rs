use anyhow::{bail, Result};
use clap::Parser;
use indicatif::{ProgressBar, ProgressState, ProgressStyle};
use rayon::prelude::*;
use regex::Regex;
use rusqlite::{params, Connection, types::ValueRef};
use sha2::Digest;
use simple_logger::SimpleLogger;
use std::sync::mpsc::{self, Receiver, Sender};
use std::{
    collections::HashMap,
    fs::File,
    hash::{Hash, Hasher},
    io::{Read, Write},
    os::raw::c_char,
    path::{Path, PathBuf},
};

use walkdir::WalkDir;

static DB_FILENAME: &str = "modland_hash.db";
static DB_REMOTE: &str = "https://www.dropbox.com/scl/fi/gtk2yri6iizlaeb6b0j0j/modland_hash.db.7z?rlkey=axcrqv54eg2c1yju6vf043ly1&dl=1";

#[repr(C)]
struct CSampleData {
    data: *const u8,
    sample_text: *const c_char,
    // length in bytes
    length_bytes: u32,
    // length in bytes
    length: u32,
    // Id for the sample in the song
    sample_id: u32,
    // Global volume (sample volume is multiplied by this), 0...64
    global_vol: u16,
    // bits per sample
    bits_per_sample: u8,
    // if stero sample or not
    stereo: u8,
    // Default sample panning (if pan flag is set), 0...256
    pan: u16,
    // Default volume, 0...256 (ignored if uFlags[SMP_NODEFAULTVOLUME] is set)
    volume: u16,
    // Frequency of middle-C, in Hz (for IT/S3M/MPTM)
    c5_speed: u32,
    // Relative note to middle c (for MOD/XM)
    relative_tone: i8,
    // Finetune period (for MOD/XM), -128...127, unit is 1/128th of a semitone
    fine_tune: i8,
    // Auto vibrato type
    vib_type: u8,
    // Auto vibrato sweep (i.e. how long it takes until the vibrato effect reaches its full depth)
    vib_sweep: u8,
    // Auto vibrato depth
    vib_depth: u8,
    // Auto vibrato rate (speed)
    vib_rate: u8,
}

impl CSampleData {
    fn get_data(&self) -> Option<&[u8]> {
        if self.data.is_null() || self.length == 0 {
            None
        } else {
            unsafe { Some(std::slice::from_raw_parts(self.data, self.length as _)) }
        }
    }

    fn get_text(&self) -> String {
        get_string_cstr(self.sample_text)
    }
}

#[repr(C)]
struct CData {
    hash: u64,
    samples: *const CSampleData,
    instrument_names: *const *const c_char,
    sample_count: u32,
    instrument_count: u32,
    channel_count: u32,
}

impl CData {
    fn get_samples(&self) -> &[CSampleData] {
        unsafe { std::slice::from_raw_parts(self.samples, self.sample_count as _) }
    }

    fn get_instrument_names(&self) -> Vec<String> {
        let mut output = Vec::new();
        for i in 0..self.instrument_count {
            let name = unsafe { get_string_cstr(*self.instrument_names.offset(i as _)) };
            output.push(name);
        }
        output
    }
}

extern "C" {
    fn hash_file(data: *const u8, len: u32, dump_patterns: i32) -> *const CData;
    fn free_hash_data(data: *const CData);
}

fn get_string_cstr(c: *const c_char) -> String {
    match unsafe { std::ffi::CStr::from_ptr(c).to_str() } {
        //Ok(s) => if s.is_empty() { String::new() } else { format!("'{}'", s.to_owned()) },
        Ok(s) => {
            let t = s.replace('\'', "''");
            format!("'{}'", t)
        }

        Err(_) => "''".to_string(),
    }
}

#[derive(Clone)]
struct SampleInfo {
    sample_id: u32,
    sha256_hash: String,
    text: String,
    length_bytes: usize,
    length: usize,
}

#[derive(Clone, Default)]
struct TrackInfo {
    pattern_hash: u64,
    sha256_hash: String,
    filename: String,
    samples: Vec<SampleInfo>,
    instrument_names: Vec<String>,
}

#[derive(Default, Debug, Clone)]
struct DatabaseMeta {
    filename: String,
    samples: Vec<String>,
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


/// Module for hashing
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Builds a new database from a given local directory
    #[clap(short, long)]
    build_database: Option<String>,

    /// Downloads the remote database (automatically performed if it doesn't exist)
    #[clap(short, long)]
    download_database: bool,

    /// Directory to search against the database. If not specified, the current directory will be used.
    #[clap(short, long, default_value = ".")]
    match_dir: String,

    /// Performs recursive scanning (includes sub-directories) when using --match-dir and --build-database
    #[clap(short, long)]
    recursive: bool,

    /// Instead of matching on hash or pattern hash match the samples in the files 
    #[clap(long)]
    match_samples: bool,

    /// Skips files with these extensions if any duplicates are found. Example: --skip-file-extensions "mdx,pdx" will skip all duplicates that contain .mdx and .pdx files (case-insensitive)
    #[clap(long, default_value = "")]
    exclude_file_extensions: String,

    /// Skips duplicates that match these paths. Example: --exclude-paths "/pub/favourites" will exclude results where "/pub/favourites" is present.
    #[clap(long, default_value = "")]
    exclude_paths: String,

    /// Includes only duplicates with these file extensions; other files will be skipped. Example: --include-file-extensions "mod,xm" will include only matches for .mod and .xm files
    #[clap(short, long, default_value = "")]
    include_file_extensions: String,

    /// Includes matches only if duplicates match these file paths. Example: --include-paths "/incoming" will show results only when at least one file matches "/incoming"
    #[clap(long, default_value = "")]
    include_paths: String,

    /// Includes matches only if one of the duplicates matches the specified regexp pattern for sample names. Example: --include_sample_name ".*ripped.*" will include duplicates where one of the tracks' sample names contains "ripped"
    #[clap(long, default_value = "")]
    include_sample_name: String,

    /// Displays duplicate results only if one of the entries includes a matching filename. Example: --search-filename ".*north.*" will include results only if one of the entries has "north" in it (case-insensitive)
    #[clap(long, default_value = "")]
    search_filename: String,

    /// Enables printing of sample names
    #[clap(short, long)]
    print_sample_names: bool,

    /// Lists existing duplicates in the database
    #[clap(short, long)]
    list_duplicates_in_database: bool,

    /// Dumps all information in the database
    #[clap(long)]
    list_database: bool,

    /// Primarily a debug option to allow dumping of pattern data when building the database and matching entries
    #[clap(long)]
    dump_patterns: bool,
}

struct Filters {
    include_paths: Vec<String>,
    include_file_extensions: Vec<String>,
    exclude_paths: Vec<String>,
    exclude_file_extensions: Vec<String>,
    sample_search: Option<Regex>,
    search_filename: Option<Regex>,
}

impl Filters {
    fn init_filter(filter: &str, prefix: &str) -> Vec<String> {
        if filter.is_empty() {
            return Vec::new();
        }

        let mut output = Vec::new();

        for t in filter.split(',') {
            output.push(format!("{}{}", prefix, t));
        }

        output
    }

    fn new(args: &Args) -> Filters {
        let sample_search = if !args.include_sample_name.is_empty() {
            Some(Regex::new(&args.include_sample_name.to_ascii_lowercase()).unwrap())
        } else {
            None
        };

        let search_filename = if !args.search_filename.is_empty() {
            Some(Regex::new(&args.search_filename.to_ascii_lowercase()).unwrap())
        } else {
            None
        };

        Filters {
            include_paths: Self::init_filter(&args.include_paths, ""),
            include_file_extensions: Self::init_filter(&args.include_file_extensions, "."),
            exclude_paths: Self::init_filter(&args.exclude_paths, ""),
            exclude_file_extensions: Self::init_filter(&args.exclude_file_extensions, "."),
            sample_search,
            search_filename,
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
    fn apply_filter(&self, input: &[DatabaseMeta], skip_level: usize) -> Vec<DatabaseMeta> {
        let mut output: Vec<DatabaseMeta> = Vec::new();

        for i in input {
            let filename = &i.filename;

            if !Self::starts_with(filename, &self.exclude_paths, false) 
                && !Self::ends_with(filename, &self.exclude_file_extensions, false)
                && Self::starts_with(filename, &self.include_paths, true)
                && Self::ends_with(filename, &self.include_file_extensions, true) 
            {
                output.push(i.clone());
            }
        }

        if let Some(re) = self.search_filename.as_ref() {
            let mut found_filename = false;

            for file in &output {
                if !file.samples.is_empty() && re.is_match(&file.filename.to_ascii_lowercase()) {
                    found_filename = true;
                    break;
                }
            }

            if !found_filename {
                return Vec::new();
            }
        }

        if let Some(re) = self.sample_search.as_ref() {
            for file in &output {
                for sample in &file.samples {
                    if re.is_match(&sample.to_ascii_lowercase()) {
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
    filename.replace('\'', "''")
    //format!("https://ftp.modland.com{}", url)
}

// Fetches info for a track/song
fn get_track_info(filename: &str, dump_patterns: bool) -> TrackInfo {
    // Calculate sha256 of the file
    let mut file = File::open(filename).unwrap();
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
        let samples = unsafe { (*song_data).get_samples() };
        track_info.pattern_hash = hash_id;

        for sample in samples {
            let sha256_hash = if let Some(data) = sample.get_data() {
                let hash = sha2::Sha256::digest(data);
                format!("'{:x}'", hash)
            } else {
                "NULL".to_string()
            };

            track_info.samples.push(SampleInfo {
                sample_id: sample.sample_id,
                sha256_hash,
                text: sample.get_text(),
                length_bytes: sample.length_bytes as _,
                length: sample.length as _,
            });
        }

        let instrument_names = unsafe { (*song_data).get_instrument_names() };

        for name in instrument_names {
            track_info.instrument_names.push(name);
        }

        //let sample_names = unsafe { get_string_cstr((*song_data).sample_names) };
        //track_info.sample_names = sample_names;
        //track_info.pattern_hash = hash_id;

        unsafe { free_hash_data(song_data) };
    }

    track_info
}

// Get the target filename
fn get_db_filename() -> String {
    let p = std::env::current_exe().unwrap();
    let path = Path::new(&p);
    let path = path.parent().unwrap().join(DB_FILENAME);
    path.into_os_string().into_string().unwrap()
}

enum DbCommand {
    Insert(String), // Example command to insert a string
    Quit,           // Example command to query a string
}

fn run_build_db_thread(filename: String, rx: Receiver<DbCommand>) -> Result<()> {
    let conn = Connection::open(filename).expect("Failed to open database");

    conn.execute("PRAGMA foreign_keys = ON", [])?;

    conn.execute(
        "CREATE TABLE files (
        song_id INTEGER PRIMARY KEY, 
        hash_id TEXT NOT NULL, 
        pattern_hash INTEGER, 
        url TEXT NOT NULL
        )",
        [],
    )
    .unwrap();

    /*
        c5_speed INTEGER,
        pan INTEGER,
        volume INTEGER,
        global_vol INTEGER,
        stereo INTEGER,
        sample_bits INTEGER,
        relative_tone INTEGER,
        fine_tune INTEGER,
        vibrato_type INTEGER,
        vibrato_sweep INTEGER,
        vibrato_depth INTEGER,
        vibrato_rate INTEGER,
    */

    conn.execute(
        "CREATE TABLE samples (
        hash_id TEXT, 
        song_id INTEGER, 
        song_sample_id INTEGER,
        text TEXT NOT NULL, 
        length_bytes INTEGER,
        length INTEGER,
        FOREIGN KEY (song_id) REFERENCES files(song_id)
        )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE instruments (
        hash_id TEXT, 
        song_id INTEGER, 
        text TEXT, 
        FOREIGN KEY (song_id) REFERENCES files(song_id)
        )",
        [],
    )?;

    conn.execute("BEGIN TRANSACTION", [])?;

    // Listen for commands
    for command in rx {
        match command {
            DbCommand::Insert(cmd) => {
                conn.execute(&cmd, [])?;
            }
            DbCommand::Quit => break,
        }
    }

    conn.execute("COMMIT", [])?;
    conn.execute("CREATE INDEX hash_files ON files (hash_id)", [])?;
    conn.execute("CREATE INDEX pattern_files ON files (pattern_hash)", [])?;
    conn.execute("CREATE INDEX hash_samples ON samples (hash_id)", [])?;
    conn.execute("CREATE INDEX length_samples ON samples (length)", [])?;
    conn.execute("CREATE INDEX song_id_samples ON samples (song_id)", [])?;

    Ok(())
}

fn build_database(out_filename: &str, database_path: &str, args: &Args) {
    // Channel for sending commands to the database thread
    let (tx, rx): (Sender<DbCommand>, Receiver<DbCommand>) = mpsc::channel();

    let filename = out_filename.to_owned();

    // Spawn the database thread
    let db_thread = std::thread::spawn(move || {
        run_build_db_thread(filename, rx).unwrap();
    });

    let files = get_files(database_path, args.recursive);

    let spinner_style =
        ProgressStyle::with_template("{prefix:.bold.dim} {wide_bar} {pos}/{len}").unwrap();

    let pb = ProgressBar::new(files.len() as _);
    pb.set_style(spinner_style);

    pb.set_prefix("Building database");

    files.par_iter().enumerate().for_each(|(index, input_path)| {
        let mut track = get_track_info(input_path, args.dump_patterns);
        track.filename = input_path.replace(database_path, "");

        let t = track.pattern_hash & 0x7FFF_FFFF_FFFF_FFFF;
        let pattern_hash = if t != 0 {
            format!("{}", t)
        } else {
            "NULL".to_string()
        };

        let insert = format!("INSERT INTO files (song_id, hash_id, pattern_hash, url) VALUES ({}, '{}', {}, '{}')", 
                index,
                &track.sha256_hash,
                pattern_hash,
                get_url(&track.filename));

         tx.send(DbCommand::Insert(insert)).expect("Failed to send command");

        for sample in &track.samples {
            let insert = format!("INSERT INTO samples (hash_id, song_id, song_sample_id, text, length_bytes, length) VALUES ({}, {}, {}, {}, {}, {})", 
                &sample.sha256_hash,
                index,
                sample.sample_id,
                &sample.text,
                sample.length_bytes,
                sample.length);

            tx.send(DbCommand::Insert(insert)).expect("Failed to send command");
        }

        pb.inc(1);
    });

    println!("Writing database...");

    tx.send(DbCommand::Quit).expect("Failed to send command");
    db_thread.join().unwrap();

    println!("Done");
}

fn create_db_file(filename: &str) -> Result<File> {
    if let Ok(file) = File::create(filename) {
        return Ok(file);
    }

    bail!(
        "Tried to create database at {} but was unable to do so. Manually download {} and place it next to the modland_has executable",
        filename, DB_REMOTE,
    )
}

fn create_progress_bar(len: usize) -> ProgressBar {
    let pb = ProgressBar::new(len as _);
    //pb.set_style(ProgressStyle::with_template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({eta})")
    pb.set_style(
        ProgressStyle::with_template(
            "{prefix} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({eta})",
        )
        .unwrap()
        .with_key(
            "eta",
            |state: &ProgressState, w: &mut dyn std::fmt::Write| {
                write!(w, "{:.1}s", state.eta().as_secs_f64()).unwrap()
            },
        )
        .progress_chars("#>-"),
    );
    pb
}

// Download and upack the database
fn download_db() -> Result<ProgressBar> {
    let filename = format!("{}.7z", get_db_filename());
    let mut file = create_db_file(&filename)?;

    let resp = ureq::get(DB_REMOTE).call()?;
    let len: usize = resp.header("Content-Length").unwrap().parse()?;

    let mut temp_buffer: [u8; 1024] = [0; 1024];
    let mut reader = resp.into_reader();

    let pb = create_progress_bar(len);

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

    Ok(pb)
}

fn decompress_db(pb: Option<ProgressBar>) -> Result<()> {
    let filename = format!("{}.7z", get_db_filename());

    // Check if compressed file exists and unpack it
    if !Path::new(&filename).exists() {
        return Ok(());
    }

    let mut sz = sevenz_rust::SevenZReader::open(&filename, "pass".into()).unwrap();
    let total_size: u64 = sz
        .archive()
        .files
        .iter()
        .filter(|e| e.has_stream())
        .map(|e| e.size())
        .sum();

    let pb = if let Some(pb) = pb {
        pb.set_length(total_size as _);
        pb
    } else {
        create_progress_bar(total_size as _)
    };

    pb.set_prefix("Decompressing Database");

    let mut uncompressed_size = 0;
    sz.for_each_entries(|_entry, reader| {
        let mut buf = [0u8; 1024];
        let mut file = File::create(get_db_filename()).unwrap();
        loop {
            let read_size = reader.read(&mut buf).unwrap();
            if read_size == 0 {
                break Ok(true);
            }
            file.write_all(&buf[..read_size])?;
            uncompressed_size += read_size;

            pb.set_position(uncompressed_size as _);
        }
    })
    .unwrap();

    // delete the compressed file
    std::fs::remove_file(&filename)?;

    Ok(())
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

fn get_samples_from_song_id(db: &Connection, song_id: u64) -> Result<Vec<String>> {
    let mut samples = Vec::new();

    let mut stmnt = db.prepare("SELECT text FROM samples WHERE song_id = :song_id")?;
    let mut rows = stmnt.query(&[(":song_id", &song_id)])?;

    while let Some(row) = rows.next()? {
        let text: String = row.get(0)?;
        samples.push(text);
    }

    Ok(samples)
}

fn get_files_from_sha_hash(info: &TrackInfo, db: &Connection) -> Result<Vec<DatabaseMeta>> {
    let mut entries = Vec::new();

    let mut stmnt = db.prepare("SELECT song_id, url FROM files WHERE hash_id = :hash")?;
    let mut rows = stmnt.query(&[(":hash", &info.sha256_hash)])?;

    while let Some(row) = rows.next()? {
        let song_id: u64 = row.get(0)?;
        let filename: String = row.get(1)?;
        let samples = get_samples_from_song_id(db, song_id)?;

        entries.push(DatabaseMeta { filename, samples });
    }

    Ok(entries)
}

fn get_files_from_pattern_hash(info: &TrackInfo, db: &Connection) -> Result<Vec<DatabaseMeta>> {
    let mut entries = Vec::new();

    if info.pattern_hash == 0 {
        return Ok(entries);
    }

    let pattern_hash = info.pattern_hash & 0x7FFF_FFFF_FFFF_FFFF;

    let mut stmnt = db.prepare("SELECT song_id, url FROM files WHERE pattern_hash = :hash")?;
    let mut rows = stmnt.query(&[(":hash", &pattern_hash)])?;

    while let Some(row) = rows.next()? {
        let song_id: u64 = row.get(0)?;
        let filename: String = row.get(1)?;
        let samples = get_samples_from_song_id(db, song_id)?;

        entries.push(DatabaseMeta { filename, samples });
    }

    Ok(entries)
}

fn print_samples_with_outline(samples: &[String], match_reg: &Option<Regex>) {
    if samples.is_empty() {
        return;
    }

    // figure out the max len of the lines
    let mut last_line_with_text = 0;
    let mut max_len = 0;
    for (index, line) in samples.iter().enumerate() {
        max_len = std::cmp::max(line.chars().count(), max_len);
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

    for (index, line) in samples.iter().enumerate() {
        print!("│ ");
        print!("{}", line);

        for _ in line.chars().count()..max_len - 1 {
            print!(" ");
        }

        if let Some(re) = match_reg.as_ref() {
            if re.is_match(&line.to_ascii_lowercase()) {
                println!("│ << regex ({}) match!", re.as_str());
            } else {
                println!("│");
            }
        } else {
            println!("│");
        }

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

fn print_found_entries(
    inital_samples: &[String],
    entries: &HashMap<&DatabaseMeta, (bool, bool)>,
    args: &Args,
    search_sample: &Option<Regex>,
) {
    let mut printed_initial_samples = false;
    let mut vals = Vec::with_capacity(entries.len());

    for found in entries {
        vals.push(found);
    }

    vals.sort_by(|a, b| a.0.filename.cmp(&b.0.filename));

    for val in &vals {
        let url = get_url(&val.0.filename);
        if args.print_sample_names {
            if !printed_initial_samples && args.print_sample_names {
                print_samples_with_outline(inital_samples, search_sample);
                printed_initial_samples = true;
            }
            println!("Found match {} (pattern_hash)", url);
            print_samples_with_outline(&val.0.samples, search_sample);
        } else if val.1 .0 && val.1 .1 {
            println!("Found match {} (hash) (pattern_hash)", url);
        } else if val.1 .0 && !val.1 .1 {
            println!("Found match {} (hash)", url);
        } else {
            println!("Found match {} (pattern_hash)", url);
        }
    }

    if vals.is_empty() {
        println!("No matches found!");
    }
}

fn match_dir_against_db(dir: &str, args: &Args, db: &Connection) -> Result<()> {
    let files = get_files(dir, args.recursive);
    let filters = Filters::new(args);

    //files.par_iter().for_each(|filename| {
    for filename in files {
        let info = get_track_info(&filename, args.dump_patterns);

        println!("Matching {}", filename);

        let filenames = get_files_from_sha_hash(&info, db)?;
        let filenames_pattern = get_files_from_pattern_hash(&info, db)?;

        let filenames = filters.apply_filter(&filenames, 1);
        let filenames_pattern = filters.apply_filter(&filenames_pattern, 1);

        let mut found_entries = HashMap::new();

        for entry in &filenames {
            found_entries.insert(entry, (true, false));
        }

        for entry in &filenames_pattern {
            if let Some(v) = found_entries.get_mut(entry) {
                v.1 = true;
            } else {
                found_entries.insert(entry, (false, true));
            }
        }

        let sample_names: Vec<String> = info.samples.iter().map(|s| s.text.to_owned()).collect();

        print_found_entries(&sample_names, &found_entries, args, &filters.sample_search);

        println!();
    }

    Ok(())
}

struct MachingSampleData {
    filename: String,
    text: String,
    sample_id: i64,
}

struct TopSampleData {
    original_sample_id: i64,
    text: String,
    matching_samples: Vec<MachingSampleData>,
}

fn match_samples(dir: &str, db: &Connection, args: &Args) -> Result<()> {
    let files = get_files(dir, args.recursive);

    for filename in files {
        let info = get_track_info(&filename, args.dump_patterns);
        let mut top_samples = Vec::new();

        if info.samples.is_empty() {
            continue;
        }
        
        let mut max_len = 0;
        for line in &info.samples {
            max_len = std::cmp::max(line.text.chars().count(), max_len);
        }

        max_len += 2;

        println!("Matching {} for duplicated samples", filename);

        for sample in &info.samples {
            let statement = format!("
                SELECT song_sample_id, text, files.url 
                FROM samples JOIN files ON samples.song_id = files.song_id WHERE samples.hash_id = {}",
                sample.sha256_hash);

            let mut stmnt = db.prepare(&statement)?;

            //println!("SELECT text FROM samples WHERE hash_id = {}", sample.sha256_hash);
            //dbg!(&sample.text, &sample.sha256_hash);

            //let mut rows = stmnt.query(params![&sample.sha256_hash])?;
            let mut rows = stmnt.query([])?;
            let mut matching_data = Vec::new();

            while let Some(row) = rows.next()? {
                let sample_id: i64 = row.get(0)?;
                let text: String = row.get(1)?;
                let url: String = row.get(2)?;

                matching_data.push(MachingSampleData {
                    filename: url,
                    text,
                    sample_id,
                });
            }

            print!("{:02} {}", sample.sample_id, &sample.text[1..sample.text.len() - 1]);

            for _ in sample.text.chars().count()..max_len - 1 {
                print!(" ");
            }

            println!("({} duplicates) length {}", matching_data.len(), sample.length);

            if !matching_data.is_empty() {
                matching_data.sort_by(|a, b| b.text.cmp(&a.text));

                let t = TopSampleData {
                    original_sample_id: sample.sample_id as _,
                    text: sample.text.to_owned(),
                    matching_samples: matching_data,
                };

                top_samples.push(t);
            }
        }

        for i in top_samples {
            println!("-------------------------------------------------------------------------------");
            println!("{:02} {}", i.original_sample_id, i.text);
            println!("-------------------------------------------------------------------------------");
            let mut max_len = 0;
            for m in &i.matching_samples {
                max_len = std::cmp::max(m.text.chars().count(), max_len);
            }

            max_len += 2;

            for m in &i.matching_samples {
                print!("{:02} {}", m.sample_id, m.text);

                for _ in m.text.chars().count()..max_len - 1 {
                    print!(" ");
                }

                println!("{}", m.filename);
            }
        }
    }

    Ok(())
}

// First check if we have a database next to the to the exe, otherwise try local directory
fn check_for_db_file() -> Option<PathBuf> {
    let path = Path::new(&get_db_filename()).to_path_buf();
    if path.exists() {
        Some(path)
    } else {
        None
    }
}

fn get_dupes(db: &Connection, args: &Args, get_songs_query: &str, get_by_id: &str, dupe_limit: usize) -> Result<Vec<Vec<DatabaseMeta>>> {
    let mut hash_dupes = Vec::with_capacity(700_0000);
    let filters = Filters::new(args);

    let mut stmnt = db.prepare(get_songs_query)?;
    let mut rows = stmnt.query([])?;

    let mut stmnt = db.prepare(get_by_id)?;

    while let Some(row) = rows.next()? {
        let v = row.get_ref(0)?;
        let mut vals = Vec::with_capacity(10);
        let mut song_ids = Vec::with_capacity(10);

        let mut song_rows = match v {
            ValueRef::Null => continue,
            ValueRef::Integer(v) => stmnt.query(params![v])?,
            ValueRef::Text(v) => stmnt.query(params![std::str::from_utf8(v)?])?,
            _ => panic!(),
        };
        
        while let Some(row) = song_rows.next()? {
            let song_id: u64 = row.get(0)?;
            let filename: String = row.get(1)?;
            let metadata = DatabaseMeta {
                filename,
                samples: Vec::new(), 
            };
            vals.push(metadata);
            song_ids.push(song_id);
        }

        if vals.len() <= dupe_limit {
            continue;
        }

        if filters.sample_search.is_some() || args.print_sample_names {
            for (metadata, song_id) in vals.iter_mut().zip(song_ids.iter()) {
                let t = get_samples_from_song_id(db, *song_id)?;
                metadata.samples = t; 
            }
        }

        let mut vals = filters.apply_filter(&vals, dupe_limit + 1);

        if !vals.is_empty() {
            vals.sort_by(|a, b| a.filename.cmp(&b.filename));
            hash_dupes.push(vals);
        }
    }

    hash_dupes.sort_by(|a, b| a[0].filename.cmp(&b[0].filename));

    Ok(hash_dupes)
}

fn print_db_duplicates(db: &Connection, args: &Args) -> Result<()> {
    let filters = Filters::new(args);

    let hash_dupes = get_dupes(
        db, args, 
        "SELECT hash_id FROM files",
        "SELECT song_id, url FROM files where hash_id = ?",
        1)?;

    let pattern_dupes = get_dupes(
        db, args, 
        "SELECT pattern_hash FROM files",
        "SELECT song_id, url FROM files where pattern_hash = ?",
        1)?;

    for (index, v) in hash_dupes.iter().enumerate() {
        println!("\n==================================================================");
        println!("Dupe Entry {} (hash)", index);

        for e in v {
            println!("{}", get_url(&e.filename));

            if filters.sample_search.is_some() || args.print_sample_names {
                print_samples_with_outline(&e.samples, &filters.sample_search);
            }
        }
    }

    for (index, v) in pattern_dupes.iter().enumerate() {
        println!("\n==================================================================");
        println!("Dupe Entry {} (pattern_hash)", index);

        for e in v {
            println!("{}", get_url(&e.filename));

            if filters.sample_search.is_some() || args.print_sample_names {
                print_samples_with_outline(&e.samples, &filters.sample_search);
            }
        }
    }

    Ok(())
}

fn print_db(db: &Connection, args: &Args) -> Result<()> {
    let filters = Filters::new(args);

    let entries = get_dupes(
        db, args, 
        "SELECT hash_id FROM files",
        "SELECT song_id, url FROM files where hash_id = ?",
        0)?;

    for (_index, v) in entries.iter().enumerate() {
        for e in v {
            println!("{}", get_url(&e.filename));

            if filters.sample_search.is_some() || args.print_sample_names {
                print_samples_with_outline(&e.samples, &filters.sample_search);
            }
        }
    }

    Ok(())
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
        let pb = download_db()?;
        decompress_db(Some(pb))?;
    } else {
        decompress_db(None)?;
    }

    let conn = Connection::open(get_db_filename())?;


    // Process duplicates in the database
    if args.list_duplicates_in_database {
        return print_db_duplicates(&conn, &args);
    }

    if args.match_samples {
        return match_samples(&args.match_dir, &conn, &args);
    }

    // Process duplicates in the database
    if args.list_database {
        return print_db(&conn, &args);
    }

    match_dir_against_db(&args.match_dir, &args, &conn)
}
