use anyhow::{bail, Context, Result};
use clap::Parser;
use indicatif::{ProgressBar, ProgressState, ProgressStyle};
use rayon::prelude::*;
use regex::{Regex, RegexBuilder};
use rusqlite::{params, types::ValueRef, Connection};
use sha2::Digest;
use simple_logger::SimpleLogger;
use std::sync::mpsc::{self, Receiver, Sender};
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    hash::{Hash, Hasher},
    io::{Read, Write},
    os::raw::{c_char, c_int},
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
            // SAFETY: `data` is non-null and points at `length` bytes owned by the module,
            // which outlives `self` (freed only by free_hash_data after use).
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
    sample_count: c_int,
    instrument_count: c_int,
    channel_count: c_int,
}

impl CData {
    fn get_samples(&self) -> &[CSampleData] {
        let count = usize::try_from(self.sample_count).unwrap_or(0);
        if self.samples.is_null() || count == 0 {
            return &[];
        }
        // SAFETY: `samples` is non-null and hash_file allocated `sample_count` initialized
        // entries, which stay valid until free_hash_data.
        unsafe { std::slice::from_raw_parts(self.samples, count) }
    }

    fn get_instrument_names(&self) -> Vec<String> {
        let count = usize::try_from(self.instrument_count).unwrap_or(0);
        if self.instrument_names.is_null() || count == 0 {
            return Vec::new();
        }
        // SAFETY: `instrument_names` is non-null and hash_file allocated `instrument_count`
        // initialized pointers, which stay valid until free_hash_data.
        let names = unsafe { std::slice::from_raw_parts(self.instrument_names, count) };
        names.iter().map(|&name| get_string_cstr(name)).collect()
    }
}

extern "C" {
    fn hash_file(data: *const u8, len: usize, dump_patterns: i32) -> *const CData;
    fn free_hash_data(data: *const CData);
}

fn get_string_cstr(c: *const c_char) -> String {
    if c.is_null() {
        return String::new();
    }
    // SAFETY: `c` is non-null and points at a NUL-terminated string owned by the module.
    match unsafe { std::ffi::CStr::from_ptr(c).to_str() } {
        Ok(s) => s.to_owned(),
        Err(_) => String::new(),
    }
}

#[derive(Clone)]
struct SampleInfo {
    sample_id: u32,
    sha256_hash: Option<String>,
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

    /// Search the database for samples matching a certain length (length is in samples)
    #[clap(long)]
    find_samples_with_length: Option<usize>,

    /// Search the database for samples matching a certain length (length is in bytes)
    #[clap(long)]
    find_samples_with_length_bytes: Option<usize>,

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

    /// Includes matches only if one of the duplicates matches the specified regexp pattern for sample names. Example: --include-sample-name ".*ripped.*" will include duplicates where one of the tracks' sample names contains "ripped"
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
        filter
            .split(',')
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(|t| format!("{}{}", prefix, t))
            .collect()
    }

    // Stored URLs are percent-encoded, so path filters must be too.
    fn init_path_filter(filter: &str) -> Vec<String> {
        Self::init_filter(&filter.to_ascii_lowercase(), "")
            .iter()
            .map(|p| get_stored_url(p))
            .collect()
    }

    fn init_regex(pattern: &str) -> Result<Option<Regex>> {
        if pattern.is_empty() {
            return Ok(None);
        }

        let re = RegexBuilder::new(pattern)
            .case_insensitive(true)
            .build()
            .with_context(|| format!("Invalid regex \"{}\"", pattern))?;

        Ok(Some(re))
    }

    fn new(args: &Args) -> Result<Filters> {
        Ok(Filters {
            include_paths: Self::init_path_filter(&args.include_paths),
            include_file_extensions: Self::init_filter(
                &args.include_file_extensions.to_ascii_lowercase(),
                ".",
            ),
            exclude_paths: Self::init_path_filter(&args.exclude_paths),
            exclude_file_extensions: Self::init_filter(
                &args.exclude_file_extensions.to_ascii_lowercase(),
                ".",
            ),
            sample_search: Self::init_regex(&args.include_sample_name)?,
            search_filename: Self::init_regex(&args.search_filename)?,
        })
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
            let filename = i.filename.to_ascii_lowercase();

            if !Self::starts_with(&filename, &self.exclude_paths, false)
                && !Self::ends_with(&filename, &self.exclude_file_extensions, false)
                && Self::starts_with(&filename, &self.include_paths, true)
                && Self::ends_with(&filename, &self.include_file_extensions, true)
            {
                output.push(i.clone());
            }
        }

        if let Some(re) = self.search_filename.as_ref() {
            let mut found_filename = false;

            for file in &output {
                if re.is_match(&file.filename) {
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
                    if re.is_match(sample) {
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
    match std::fs::metadata(path) {
        Ok(md) if md.is_file() => return vec![path.to_owned()],
        Ok(_) => {}
        Err(e) => {
            eprintln!("Skipping {}: {}", path, e);
            return Vec::new();
        }
    }

    let _spinner_style = ProgressStyle::with_template("{prefix:.bold.dim} {spinner} {wide_msg}")
        .unwrap()
        .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ ");

    //let pb = ProgressBar::new(0);
    //pb.set_style(spinner_style);
    //pb.set_prefix(format!("Fetching list of files... [{}/?]", 0));

    let max_depth = if !recurse { 1 } else { usize::MAX };

    let files: Vec<String> = WalkDir::new(path)
        .max_depth(max_depth)
        .into_iter()
        .filter_map(|e| {
            let file = match e {
                Ok(file) => file,
                Err(e) => {
                    let path = e.path().unwrap_or(Path::new(path));
                    eprintln!("Skipping {}: {}", path.display(), e);
                    return None;
                }
            };

            let filename = file.path().to_str()?;
            if file.file_type().is_file() && should_scan(&file.file_name().to_string_lossy()) {
                //pb.set_message(filename.to_owned());
                return Some(filename.to_owned());
            }
            None
        })
        .collect();
    files
}

// The database and its archive live next to the executable and must not be scanned as input
fn should_scan(file_name: &str) -> bool {
    file_name != DB_FILENAME
        && file_name.strip_suffix(".7z") != Some(DB_FILENAME)
        && !file_name.ends_with(".listing")
}

fn get_url(filename: &str) -> String {
    let filename = filename.replace(' ', "%20");
    let filename = filename.replace('\'', "%27");
    format!("https://ftp.modland.com{}", filename)
}

fn get_stored_url(filename: &str) -> String {
    let filename = filename.replace(' ', "%20");
    filename.replace('\'', "%27")
}

// Fetches info for a track/song
fn get_track_info(filename: &str, dump_patterns: bool) -> Result<TrackInfo> {
    // Calculate sha256 of the file
    let file_data = std::fs::read(filename)?;
    let hash = sha2::Sha256::digest(&file_data);
    let dump_patterns = if dump_patterns { 1 } else { 0 };

    // SAFETY: the pointer/length pair describes `file_data`, which outlives the call.
    let song_data = unsafe { hash_file(file_data.as_ptr(), file_data.len(), dump_patterns) };

    let mut track_info = TrackInfo {
        filename: filename.to_owned(),
        sha256_hash: format!("{:x}", hash),
        ..Default::default()
    };

    if !song_data.is_null() {
        // SAFETY: `song_data` is non-null and stays valid until free_hash_data below.
        let song = unsafe { &*song_data };
        let hash_id = song.hash;
        let samples = song.get_samples();
        track_info.pattern_hash = hash_id;

        if hash_id == 1 {
            println!("Song with 0x1ff detected: {}", filename);
        }

        for sample in samples {
            let sha256_hash = sample
                .get_data()
                .map(|data| format!("{:x}", sha2::Sha256::digest(data)));

            track_info.samples.push(SampleInfo {
                sample_id: sample.sample_id,
                sha256_hash,
                text: sample.get_text(),
                length_bytes: sample.length_bytes as _,
                length: sample.length as _,
            });
        }

        track_info.instrument_names = song.get_instrument_names();

        // SAFETY: `song_data` came from hash_file, is freed exactly once, and `song` and
        // `samples` are not used afterwards.
        unsafe { free_hash_data(song_data) };
    }

    Ok(track_info)
}

// Get the target filename
fn get_db_filename() -> String {
    let p = std::env::current_exe().unwrap();
    let path = Path::new(&p);
    let path = path.parent().unwrap().join(DB_FILENAME);
    path.into_os_string().into_string().unwrap()
}

enum DbCommand {
    Insert { song_id: usize, track: TrackInfo },
    Quit,
}

// The DB only stores the lower 63 bits; 0 means "no pattern hash"
fn stored_pattern_hash(pattern_hash: u64) -> Option<i64> {
    let masked = pattern_hash & 0x7FFF_FFFF_FFFF_FFFF;
    (masked != 0).then_some(masked as i64)
}

fn insert_track(conn: &Connection, song_id: usize, track: &TrackInfo) -> Result<()> {
    conn.prepare_cached(
        "INSERT INTO files (song_id, hash_id, pattern_hash, url) VALUES (?1, ?2, ?3, ?4)",
    )?
    .execute(params![
        song_id,
        track.sha256_hash,
        stored_pattern_hash(track.pattern_hash),
        get_stored_url(&track.filename)
    ])?;

    let mut stmnt = conn.prepare_cached(
        "INSERT INTO samples (hash_id, song_id, song_sample_id, text, length_bytes, length) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;

    for sample in &track.samples {
        stmnt.execute(params![
            sample.sha256_hash,
            song_id,
            sample.sample_id,
            sample.text,
            sample.length_bytes,
            sample.length
        ])?;
    }

    Ok(())
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
            DbCommand::Insert { song_id, track } => insert_track(&conn, song_id, &track)?,
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

    let _spinner_style =
        ProgressStyle::with_template("{prefix:.bold.dim} {wide_bar} {pos}/{len}").unwrap();

    //let pb = ProgressBar::new(files.len() as _);
    //pb.set_style(spinner_style);

    //pb.set_prefix("Building database");

    files
        .par_iter()
        .enumerate()
        .for_each(|(index, input_path)| {
            let mut track = match get_track_info(input_path, args.dump_patterns) {
                Ok(track) => track,
                Err(e) => {
                    eprintln!("Skipping {}: {}", input_path, e);
                    return;
                }
            };
            track.filename = input_path.replace(database_path, "");

            tx.send(DbCommand::Insert {
                song_id: index,
                track,
            })
            .expect("Failed to send command");

            //pb.inc(1);
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

    let Some(pattern_hash) = stored_pattern_hash(info.pattern_hash) else {
        return Ok(entries);
    };

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
            if re.is_match(line) {
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

struct MatchKind {
    hash: bool,
    pattern_hash: bool,
}

impl MatchKind {
    fn label(&self) -> &'static str {
        match (self.hash, self.pattern_hash) {
            (true, true) => "(hash) (pattern_hash)",
            (true, false) => "(hash)",
            _ => "(pattern_hash)",
        }
    }
}

fn print_found_entries(
    inital_samples: &[String],
    entries: &HashMap<&DatabaseMeta, MatchKind>,
    args: &Args,
    search_sample: &Option<Regex>,
) {
    let mut printed_initial_samples = false;
    let mut vals = Vec::with_capacity(entries.len());

    for found in entries {
        vals.push(found);
    }

    vals.sort_by(|a, b| a.0.filename.cmp(&b.0.filename));

    for (meta, kind) in &vals {
        let url = get_url(&meta.filename);
        if args.print_sample_names && !printed_initial_samples {
            print_samples_with_outline(inital_samples, search_sample);
            printed_initial_samples = true;
        }
        println!("Found match {} {}", url, kind.label());
        if args.print_sample_names {
            print_samples_with_outline(&meta.samples, search_sample);
        }
    }

    if vals.is_empty() {
        println!("No matches found!");
    }
}

fn match_dir_against_db(dir: &str, args: &Args, filters: &Filters, db: &Connection) -> Result<()> {
    let files = get_files(dir, args.recursive);

    //files.par_iter().for_each(|filename| {
    for filename in files {
        let info = match get_track_info(&filename, args.dump_patterns) {
            Ok(info) => info,
            Err(e) => {
                eprintln!("Skipping {}: {}", filename, e);
                continue;
            }
        };

        println!("Matching {}", filename);

        let filenames = get_files_from_sha_hash(&info, db)?;
        let filenames_pattern = get_files_from_pattern_hash(&info, db)?;

        let filenames = filters.apply_filter(&filenames, 1);
        let filenames_pattern = filters.apply_filter(&filenames_pattern, 1);

        let mut found_entries = HashMap::new();

        for entry in &filenames {
            found_entries.insert(
                entry,
                MatchKind {
                    hash: true,
                    pattern_hash: false,
                },
            );
        }

        for entry in &filenames_pattern {
            found_entries
                .entry(entry)
                .or_insert(MatchKind {
                    hash: false,
                    pattern_hash: true,
                })
                .pattern_hash = true;
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
    text_lower: String,
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
        let info = match get_track_info(&filename, args.dump_patterns) {
            Ok(info) => info,
            Err(e) => {
                eprintln!("Skipping {}: {}", filename, e);
                continue;
            }
        };
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
            let mut matching_data = Vec::new();

            if sample.length_bytes > 0 {
                let mut stmnt = db.prepare(
                    "SELECT song_sample_id, text, files.url
                    FROM samples JOIN files ON samples.song_id = files.song_id WHERE samples.hash_id = ?1",
                )?;
                let mut rows = stmnt.query([&sample.sha256_hash])?;

                while let Some(row) = rows.next()? {
                    let sample_id: i64 = row.get(0)?;
                    let text: String = row.get(1)?;
                    let url: String = row.get(2)?;
                    let text_lower = text.to_ascii_lowercase();

                    matching_data.push(MachingSampleData {
                        filename: url,
                        text,
                        text_lower,
                        sample_id,
                    });
                }
            }

            print!("{:02} {}", sample.sample_id, sample.text);

            for _ in sample.text.chars().count()..max_len - 1 {
                print!(" ");
            }

            if !matching_data.is_empty() {
                println!(
                    "({} duplicates) length {}",
                    matching_data.len(),
                    sample.length
                );
            } else {
                println!("length {}", sample.length);
            }

            if !matching_data.is_empty() {
                matching_data.sort_by(|a, b| b.text_lower.cmp(&a.text_lower));

                let t = TopSampleData {
                    original_sample_id: sample.sample_id as _,
                    text: sample.text.to_owned(),
                    matching_samples: matching_data,
                };

                top_samples.push(t);
            }
        }

        for i in top_samples {
            println!(
                "-------------------------------------------------------------------------------"
            );
            println!("{:02} {}", i.original_sample_id, i.text);
            println!(
                "-------------------------------------------------------------------------------"
            );
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

fn get_dupes(
    db: &Connection,
    args: &Args,
    filters: &Filters,
    get_songs_query: &str,
    get_by_id: &str,
    dupe_limit: usize,
) -> Result<Vec<Vec<DatabaseMeta>>> {
    let mut hash_dupes = Vec::with_capacity(700_0000);

    let mut stmnt = db.prepare(get_songs_query)?;
    let mut rows = stmnt.query([])?;

    let mut stmnt = db.prepare(get_by_id)?;
    let mut hash_id_lookup_string = HashSet::new();
    let mut hash_id_lookup_int = HashSet::new();

    while let Some(row) = rows.next()? {
        let v = row.get_ref(0)?;
        let mut vals = Vec::with_capacity(10);
        let mut song_ids = Vec::with_capacity(10);

        let mut song_rows = match v {
            ValueRef::Null => continue,
            ValueRef::Integer(v) => {
                let v = v as u64;
                if let Some(_v) = hash_id_lookup_int.get(&v) {
                    continue;
                } else {
                    hash_id_lookup_int.insert(v);
                }

                stmnt.query(params![v])?
            }

            ValueRef::Text(v) => {
                let v = std::str::from_utf8(v)?.to_owned();

                if let Some(_v) = hash_id_lookup_string.get(&v) {
                    continue;
                } else {
                    hash_id_lookup_string.insert(v.clone());
                }

                stmnt.query(params![v])?
            }

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

fn print_db_duplicates(db: &Connection, args: &Args, filters: &Filters) -> Result<()> {
    let hash_dupes = get_dupes(
        db,
        args,
        filters,
        "SELECT hash_id FROM files",
        "SELECT song_id, url FROM files where hash_id = ?",
        1,
    )?;

    let pattern_dupes = get_dupes(
        db,
        args,
        filters,
        "SELECT pattern_hash FROM files",
        "SELECT song_id, url FROM files where pattern_hash = ?",
        1,
    )?;

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

fn print_db(db: &Connection, args: &Args, filters: &Filters) -> Result<()> {
    let entries = get_dupes(
        db,
        args,
        filters,
        "SELECT hash_id FROM files",
        "SELECT song_id, url FROM files where hash_id = ?",
        0,
    )?;

    for v in entries.iter() {
        for e in v {
            println!("{}", get_url(&e.filename));

            if filters.sample_search.is_some() || args.print_sample_names {
                print_samples_with_outline(&e.samples, &filters.sample_search);
            }
        }
    }

    Ok(())
}

fn print_sample_rows(rows: &mut rusqlite::Rows, filters: &Filters) -> Result<()> {
    let mut data = Vec::with_capacity(1024);
    let mut max_len = 0;

    while let Some(row) = rows.next()? {
        let sample_id: i64 = row.get(0)?;
        let text: String = row.get(1)?;
        let url: String = row.get(2)?;

        if let Some(re) = filters.sample_search.as_ref() {
            if !re.is_match(&text) {
                continue;
            }
        }

        max_len = std::cmp::max(text.chars().count(), max_len);
        let text_lower = text.to_ascii_lowercase();

        data.push(MachingSampleData {
            filename: url,
            text,
            text_lower,
            sample_id,
        });
    }

    if data.is_empty() {
        println!("No matches found!");
        return Ok(());
    }

    println!("Found {} samples matching", data.len());

    data.sort_by(|a, b| b.text_lower.cmp(&a.text_lower));

    for d in data {
        print!("{:03} {}", d.sample_id, d.text);

        for _ in d.text.chars().count()..max_len {
            print!(" ");
        }

        println!("{}", d.filename);
    }

    Ok(())
}

fn match_db_with_sample_length(db: &Connection, filters: &Filters, length: usize) -> Result<()> {
    let mut stmnt = db.prepare(
        "SELECT song_sample_id, text, files.url
        FROM samples JOIN files ON samples.song_id = files.song_id WHERE samples.length = ?1",
    )?;
    let mut rows = stmnt.query([length])?;

    print_sample_rows(&mut rows, filters)
}

fn match_db_with_sample_length_bytes(
    db: &Connection,
    filters: &Filters,
    length: usize,
) -> Result<()> {
    let mut stmnt = db.prepare(
        "SELECT song_sample_id, text, files.url
        FROM samples JOIN files ON samples.song_id = files.song_id WHERE samples.length_bytes = ?1",
    )?;
    let mut rows = stmnt.query([length])?;

    print_sample_rows(&mut rows, filters)
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

    let filters = Filters::new(&args)?;

    let database_path = check_for_db_file();

    if args.download_database || database_path.is_none() {
        let pb = download_db()?;
        decompress_db(Some(pb))?;
    } else {
        decompress_db(None)?;
    }

    let conn = Connection::open(get_db_filename())?;

    if let Some(len) = args.find_samples_with_length {
        return match_db_with_sample_length(&conn, &filters, len);
    }

    if let Some(len) = args.find_samples_with_length_bytes {
        return match_db_with_sample_length_bytes(&conn, &filters, len);
    }

    // Process duplicates in the database
    if args.list_duplicates_in_database {
        return print_db_duplicates(&conn, &args, &filters);
    }

    if args.match_samples {
        return match_samples(&args.match_dir, &conn, &args);
    }

    // Process duplicates in the database
    if args.list_database {
        return print_db(&conn, &args, &filters);
    }

    match_dir_against_db(&args.match_dir, &args, &filters, &conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDb(PathBuf);

    impl Drop for TempDb {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn sample(id: u32, hash: Option<&str>, text: &str) -> SampleInfo {
        SampleInfo {
            sample_id: id,
            sha256_hash: hash.map(str::to_owned),
            text: text.to_owned(),
            length_bytes: 10 + id as usize,
            length: 5 + id as usize,
        }
    }

    fn build_test_db(db_path: &Path, tracks: Vec<TrackInfo>) {
        let (tx, rx) = mpsc::channel();
        for (song_id, track) in tracks.into_iter().enumerate() {
            tx.send(DbCommand::Insert { song_id, track }).unwrap();
        }
        tx.send(DbCommand::Quit).unwrap();
        run_build_db_thread(db_path.to_str().unwrap().to_owned(), rx).unwrap();
    }

    #[test]
    fn build_db_stores_raw_values() {
        let guard = TempDb(std::env::temp_dir().join(format!(
            "modland_hash_test_{}_raw_values.db",
            std::process::id()
        )));
        let _ = std::fs::remove_file(&guard.0);

        let with_pattern = TrackInfo {
            pattern_hash: 0x8000_0000_0000_0007,
            sha256_hash: "aa".to_owned(),
            filename: "/pub/mods/it's a song.mod".to_owned(),
            samples: vec![sample(0, Some("bb"), "it's here"), sample(1, None, "")],
            instrument_names: Vec::new(),
        };
        let no_pattern = TrackInfo {
            sha256_hash: "cc".to_owned(),
            filename: "/pub/mods/b.mod".to_owned(),
            ..Default::default()
        };

        build_test_db(&guard.0, vec![with_pattern, no_pattern]);

        let conn = Connection::open(&guard.0).unwrap();

        let files: Vec<(i64, String, Option<i64>, String)> = conn
            .prepare("SELECT song_id, hash_id, pattern_hash, url FROM files ORDER BY song_id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            files,
            vec![
                (
                    0,
                    "aa".to_owned(),
                    Some(7),
                    "/pub/mods/it%27s%20a%20song.mod".to_owned()
                ),
                (1, "cc".to_owned(), None, "/pub/mods/b.mod".to_owned()),
            ]
        );

        let samples: Vec<(Option<String>, i64, i64, String, i64, i64)> = conn
            .prepare(
                "SELECT hash_id, song_id, song_sample_id, text, length_bytes, length
                 FROM samples ORDER BY song_sample_id",
            )
            .unwrap()
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            })
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            samples,
            vec![
                (Some("bb".to_owned()), 0, 0, "it's here".to_owned(), 10, 5),
                (None, 0, 1, String::new(), 11, 6),
            ]
        );
    }

    struct TempFile(PathBuf);

    impl TempFile {
        fn create(name: &str, bytes: &[u8]) -> Self {
            let path = std::env::temp_dir().join(format!(
                "modland_hash_test_{}_{}",
                std::process::id(),
                name
            ));
            std::fs::write(&path, bytes).unwrap();
            Self(path)
        }

        fn track_info(&self) -> TrackInfo {
            get_track_info(self.0.to_str().unwrap(), false).unwrap()
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn minimal_mod() -> Vec<u8> {
        let mut bytes = vec![0u8; 20];
        for i in 0..31 {
            let mut header = [0u8; 30];
            header[22..24].copy_from_slice(&(if i == 0 { 4u16 } else { 0 }).to_be_bytes());
            header[25] = 64;
            header[28..30].copy_from_slice(&1u16.to_be_bytes());
            bytes.extend_from_slice(&header);
        }
        bytes.push(1);
        bytes.push(127);
        bytes.extend_from_slice(&[0u8; 128]);
        bytes.extend_from_slice(b"M.K.");
        let mut pattern = vec![0u8; 1024];
        pattern[..4].copy_from_slice(&[0x01, 0xAC, 0x10, 0x00]);
        bytes.extend_from_slice(&pattern);
        bytes.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        bytes
    }

    struct TempDir(PathBuf);

    impl TempDir {
        fn create(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "{}_test_{}_{}",
                DB_FILENAME.trim_end_matches(".db"),
                std::process::id(),
                name
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn write(&self, rel: &str, bytes: &[u8]) {
            let path = self.0.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }

        fn scan(&self, recurse: bool) -> Vec<String> {
            let root = self.0.to_str().unwrap();
            let mut files: Vec<String> = get_files(root, recurse)
                .into_iter()
                .map(|f| {
                    f.strip_prefix(root)
                        .unwrap()
                        .trim_start_matches('/')
                        .to_owned()
                })
                .collect();
            files.sort();
            files
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn get_files_scans_dirs_with_modland_hash_in_path() {
        let dir = TempDir::create("in_path");
        assert!(dir.0.to_str().unwrap().contains("modland_hash"));
        dir.write("a.mod", b"a");
        assert_eq!(dir.scan(false), vec!["a.mod"]);
    }

    #[test]
    fn get_files_skips_database_and_listing_files() {
        let dir = TempDir::create("skips");
        dir.write("a.mod", b"a");
        dir.write(DB_FILENAME, b"x");
        dir.write(&format!("{}.7z", DB_FILENAME), b"x");
        dir.write("x.listing", b"x");
        dir.write("my_modland_hash.mod", b"x");
        assert_eq!(dir.scan(false), vec!["a.mod", "my_modland_hash.mod"]);
    }

    #[test]
    fn get_files_respects_recursion() {
        let dir = TempDir::create("depth");
        dir.write("a.mod", b"a");
        dir.write("sub/b.mod", b"b");
        assert_eq!(dir.scan(false), vec!["a.mod"]);
        assert_eq!(dir.scan(true), vec!["a.mod", "sub/b.mod"]);
    }

    #[test]
    fn get_files_of_missing_path_is_empty() {
        let dir = TempDir::create("missing");
        let missing = dir.0.join("nope");
        assert!(get_files(missing.to_str().unwrap(), true).is_empty());
    }

    #[test]
    fn track_info_of_missing_file_is_err() {
        let dir = TempDir::create("missing_track");
        let missing = dir.0.join("nope.mod");
        assert!(get_track_info(missing.to_str().unwrap(), false).is_err());
    }

    #[test]
    fn track_info_of_non_module_is_empty() {
        let file = TempFile::create("not_a_module.bin", &[0x55; 64]);
        let info = file.track_info();
        assert_eq!(info.pattern_hash, 0);
        assert!(info.samples.is_empty());
        assert!(info.instrument_names.is_empty());
    }

    fn filters_from(extra: &[&str]) -> Result<Filters> {
        let args = Args::parse_from(["modland_hash"].iter().chain(extra));
        Filters::new(&args)
    }

    fn meta(filename: &str, samples: &[&str]) -> DatabaseMeta {
        DatabaseMeta {
            filename: filename.to_owned(),
            samples: samples.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn filtered_names(filters: &Filters, input: &[DatabaseMeta]) -> Vec<String> {
        filters
            .apply_filter(input, 1)
            .into_iter()
            .map(|m| m.filename)
            .collect()
    }

    #[test]
    fn sample_regex_keeps_escape_classes_and_ignores_case() {
        let filters = filters_from(&["--include-sample-name", r"ahh\Sox"]).unwrap();
        let input = [meta("/a.mod", &["st-91:AHHVOX"])];
        assert_eq!(filtered_names(&filters, &input), vec!["/a.mod"]);

        let filters = filters_from(&["--include-sample-name", r"ahh\sox"]).unwrap();
        assert!(filtered_names(&filters, &input).is_empty());
    }

    #[test]
    fn filename_regex_keeps_escape_classes_and_ignores_case() {
        let filters = filters_from(&["--search-filename", r"^/pub/\w+/Foo"]).unwrap();
        let input = [meta("/pub/mods/FOO.mod", &[])];
        assert_eq!(filtered_names(&filters, &input), vec!["/pub/mods/FOO.mod"]);
    }

    #[test]
    fn invalid_regex_is_an_error_naming_the_pattern() {
        let err = filters_from(&["--include-sample-name", "(unclosed"])
            .err()
            .unwrap();
        assert!(err.to_string().contains("(unclosed"));
        assert!(filters_from(&["--search-filename", "[bad"]).is_err());
    }

    #[test]
    fn path_filters_match_percent_encoded_urls() {
        let input = [
            meta("/pub/modules/Protracker/Richard%20Joseph/x.mod", &[]),
            meta("/pub/modules/Protracker/Other/y.mod", &[]),
        ];

        let include =
            filters_from(&["--include-paths", "/pub/modules/Protracker/Richard Joseph"]).unwrap();
        assert_eq!(
            filtered_names(&include, &input),
            vec!["/pub/modules/Protracker/Richard%20Joseph/x.mod"]
        );

        let exclude =
            filters_from(&["--exclude-paths", "/pub/modules/Protracker/Richard Joseph"]).unwrap();
        assert_eq!(
            filtered_names(&exclude, &input),
            vec!["/pub/modules/Protracker/Other/y.mod"]
        );
    }

    #[test]
    fn filter_lists_are_trimmed_and_drop_empty_items() {
        let filters = filters_from(&["--include-file-extensions", "mod, xm,,"]).unwrap();
        assert_eq!(filters.include_file_extensions, vec![".mod", ".xm"]);

        let filters = filters_from(&["--include-paths", " /a , ,/b"]).unwrap();
        assert_eq!(filters.include_paths, vec!["/a", "/b"]);
    }

    #[test]
    fn track_info_of_minimal_mod_has_samples_and_hash() {
        let file = TempFile::create("minimal.mod", &minimal_mod());
        let info = file.track_info();
        assert_ne!(info.pattern_hash, 0);
        assert_eq!(info.samples.len(), 31);
    }

    #[test]
    fn match_kind_labels() {
        let label = |hash, pattern_hash| MatchKind { hash, pattern_hash }.label();
        assert_eq!(label(true, true), "(hash) (pattern_hash)");
        assert_eq!(label(true, false), "(hash)");
        assert_eq!(label(false, true), "(pattern_hash)");
    }
}
