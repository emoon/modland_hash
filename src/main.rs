use clap::Parser;
use filetime::FileTime;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::io::Read;
use std::io::Write;
use std::os::raw::c_char;
use std::sync::Mutex;
use walkdir::WalkDir;

fn get_files(path: &str) -> Vec<(String, i64)> {
    let spinner_style = ProgressStyle::with_template("{prefix:.bold.dim} {spinner} {wide_msg}")
        .unwrap()
        .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ ");

    let pb = ProgressBar::new(0);
    pb.set_style(spinner_style.clone());
    pb.set_prefix(format!("Fetching list of files... [{}/?]", 0));

    let files: Vec<(String, i64)> = WalkDir::new(path)
        .into_iter()
        .filter_map(|e| {
            let file = e.unwrap();
            let metadata = file.metadata().unwrap();
            let mtime = FileTime::from_last_modification_time(&metadata);

            if let Some(filename) = file.path().to_str() {
                if metadata.is_file() {
                    return Some((file, mtime.seconds()));
                }
            }

            None
        })
        .map(|(entry, time)| {
            let filename = entry.into_path().to_str().unwrap().to_string();
            pb.set_message(format!("{}", &filename));
            //pb.inc(1);
            (filename, time)
        })
        .collect();
    files
}

#[derive(Serialize, Clone, Deserialize, Hash, Eq, PartialEq, Default, Debug)]
struct Hash {
    v0: u64,
    v1: u64,
    v2: u64,
    v3: u64,
}

fn get_u64_from_u8(data: &[u8]) -> u64 {
    let mut o = 0u64;

    for v in data {
        let t = *v as u64;
        o |= t;
        o <<= 8;
    }

    o
}

#[repr(C)]
struct CData {
    hash: u64,
    sample_names: *const c_char,
    artist: *const c_char,
    comments: *const c_char,
    channel_count: i32,
}

extern "C" {
    fn hash_file(filename: *const i8) -> *const CData;
    fn free_hash_data(data: *const CData);
}

#[derive(Clone, Serialize, Deserialize, Default)]
struct SongMetadata {
    filename: String,
    sample_names: String,
    artist: String,
    comments: String,
    channel_count: i32,
}

fn get_string_cstr(c: *const c_char) -> String {
    unsafe { std::ffi::CStr::from_ptr(c).to_string_lossy().into_owned() }
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct TrackInfo {
    timestamp: i64,
    pattern_hash: u64,
    sha256_hash: Hash,
    filename: String,
    metadata: Option<SongMetadata>,
}

// Holds a cache of all the files so we don't need to rehash them all the time
#[derive(Clone, Default, Serialize, Deserialize)]
struct ModlandCache {
    data: Vec<TrackInfo>,
    /*
    filename_to_data: HashMap<String, usize>,
    sha256_to_data: HashMap<Hash, usize>,
    pattern_hash_to_data: HashMap<u64, usize>,
    */
}

/// Simple program to greet a person
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Name of the person to greet
    #[clap(short, long)]
    ignore_file_types: Vec<String>,

    // Make it possible to skip the database update in case you know it's up to date (only cache will be used)
    #[clap(short, long)]
    skip_database_update: bool,

    /// Path to local copy of the modland files
    #[clap(short, long)]
    database: String,

    /// Search regex in samples
    #[clap(short, long)]
    match_sample: Option<String>,
}

/// Loads cache from disk if one exists and is then used
fn load_cache_from_disk(filename: &str) -> Vec<TrackInfo> {
    if let Ok(mut f) = File::open(filename) {
        println!("Reading cache: {} to memory", filename);
        let mut file_data = Vec::with_capacity(200 * 1024 * 1024);
        f.read_to_end(&mut file_data).unwrap();
        let decoded: Vec<TrackInfo> = bincode::deserialize(&file_data[..]).unwrap();
        println!("Reading cache: {} to memory (done)", filename);
        decoded
    } else {
        Vec::new()
    }
}

// Updates the database with new entries
fn update_database(filepath: &str, existing_data: &mut Vec<TrackInfo>) -> bool {
    let files = get_files(filepath);

    // Build a hashmap for matching existing files with timestamp. If timestamp is the same we assume the file is unchanged
    let mut filename_timestamp_lookup = HashMap::with_capacity(existing_data.len());

    for (index, track) in existing_data.iter().enumerate() {
        filename_timestamp_lookup.insert(track.filename.to_owned(), (track.timestamp, index));
    }

    println!("Updating database");
    let pb = ProgressBar::new(files.len() as _);
    let data = Mutex::new(Vec::new());

    files
        .par_iter()
        .enumerate()
        .for_each(|(_file_id, input_path)| {
            let song_data;

            // Check if the file has unchanged timestamp we assume it's unchanged and in the cache
            if let Some(time_stamp) = filename_timestamp_lookup.get(&input_path.0) {
                if time_stamp.0 == input_path.1 {
                    pb.inc(1);
                    return;
                }
            }

            {
                let c_filename = std::ffi::CString::new(input_path.0.as_bytes()).unwrap();
                song_data = unsafe { hash_file(c_filename.as_ptr()) };
            }

            pb.inc(1);

            // Calculate sha256 of the file
            let file = std::fs::File::open(&input_path.0).unwrap();
            let mut reader = BufReader::new(file);
            let mut sha256 = Sha256::new();
            let mut file_data = Vec::new();
            reader.read_to_end(&mut file_data).unwrap();
            sha256.update(&file_data);
            let hash = sha256.finalize();

            let hash = Hash {
                v0: get_u64_from_u8(&hash[0..7]),
                v1: get_u64_from_u8(&hash[8..15]),
                v2: get_u64_from_u8(&hash[16..23]),
                v3: get_u64_from_u8(&hash[24..31]),
            };

            let mut track_info = TrackInfo::default();
            track_info.filename = input_path.0.to_owned();
            track_info.timestamp = input_path.1;
            track_info.sha256_hash = hash;

            if song_data != std::ptr::null() {
                let hash_id = unsafe { (*song_data).hash };
                let metadata = unsafe {
                    SongMetadata {
                        filename: input_path.0.to_owned(),
                        sample_names: get_string_cstr((*song_data).sample_names),
                        artist: get_string_cstr((*song_data).artist),
                        comments: get_string_cstr((*song_data).comments),
                        channel_count: (*song_data).channel_count,
                    }
                };

                track_info.metadata = Some(metadata);
                track_info.pattern_hash = hash_id;

                unsafe { free_hash_data(song_data) };
            }

            {
                let mut tracks = data.lock().unwrap();
                tracks.push(track_info);
            }
        });

    let new_data = data.lock().unwrap();

    // if data was epmty we just push the newly generated data into it

    if existing_data.is_empty() {
        *existing_data = new_data.clone();
        true
    } else if !new_data.is_empty() {
        // loop over the new entries or either update them or add new entries
        for e in &*new_data {
            // check if entry was found in existing data, if so we update with the new data
            if let Some(time_stamp) = filename_timestamp_lookup.get(&e.filename) {
                existing_data[time_stamp.1] = e.clone();
            } else {
                // otherwise push to the list
                existing_data.push(e.clone());
            }
        }

        true
    } else {
        false
    }
}

fn search_for_sample_name(search_string: &str, tracks: &[TrackInfo]) {
    let re = Regex::new(search_string).unwrap();
    let mut count = 0;

    tracks.iter().for_each(|track| {
        if let Some(metadata) = track.metadata.as_ref() {
            if re.is_match(&metadata.sample_names) {
                println!("===============================================================");
                println!("Matching {}", track.filename);
                println!("{}", metadata.sample_names);
                count += 1;
            }
        }
    });

    println!("Total matches {}", count);
}

// tetsehou
fn main() {
    let args = Args::parse();
    let cache_filename = "cache.bin";

    let mut track_info = load_cache_from_disk(cache_filename);

    if !args.skip_database_update {
        let is_updated = update_database(&args.database, &mut track_info);

        // store new cache in case it was updated
        if is_updated {
            println!("Database has been updated. Storing cache...");
            let encoded: Vec<u8> = bincode::serialize(&track_info).unwrap();
            let mut cache = File::create(cache_filename).unwrap();
            cache.write_all(&encoded).unwrap();
            println!("Database has been updated. Storing cache... (Done)");
        }
    }

    if let Some(match_sample) = args.match_sample {
        search_for_sample_name(&match_sample, &track_info);
        return;
    }
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
}

dupe_array.sort_by(|a, b| a[0].filename.cmp(&b[0].filename));

for val in dupe_array {
    let mut found_unknown = false;

    for t in val {
        if t.filename.contains("- unknown") {
            found_unknown = true;
        }

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

/*
let mut data = Vec::new();
let mut file = std::fs::File::open(input_path).unwrap();
file.read_to_end(&mut data).unwrap();

if data.len() >= 7 {
    let len = data.len() - 7;

    for i in 0..len {
        let range = &data[i..i + 7];

        /*
        if range[0] == b'<'
            && range[1] == b'S'
            && range[2] == b'C'
            && range[3] == b'R'
            && range[4] == b'I'
            && range[5] == b'P'
            && range[6] == b'T'
        {
            println!("{}", &input_path[18..]);
            break;
        Write}
        */

        if range[0] == b'<'
            && range[1] == b's'
            && range[2] == b'c'
            && range[3] == b'r'
            && range[4] == b'i'
            && range[5] == b'p'
            && range[6] == b't'
        {
            println!("{}", &input_path[18..]);
            break;
        }

    }
}
*/
