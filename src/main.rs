use anyhow::Result;
use clap::Parser;
//use filetime::FileTime;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
//use regex::Regex;
use rusqlite::{Connection, Statement};
use sha2::Digest;
use std::{collections::HashMap, io::Read, os::raw::c_char, sync::Mutex};
use walkdir::WalkDir;

fn get_files(path: &str) -> Vec<String> {
    let spinner_style = ProgressStyle::with_template("{prefix:.bold.dim} {spinner} {wide_msg}")
        .unwrap()
        .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ ");

    let pb = ProgressBar::new(0);
    pb.set_style(spinner_style);
    pb.set_prefix(format!("Fetching list of files... [{}/?]", 0));

    let files: Vec<String> = WalkDir::new(path)
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

#[derive(Clone, Default)]
struct SongMetadata {
    sample_names: String,
    //artist: String,
    //comments: String,
    //channel_count: i32,
}

fn get_string_cstr(c: *const c_char) -> String {
    unsafe { std::ffi::CStr::from_ptr(c).to_string_lossy().into_owned() }
}

#[derive(Clone, Default)]
struct TrackInfo {
    pattern_hash: u64,
    sha256_hash: String,
    filename: String,
    metadata: Option<SongMetadata>,
}

/// Simple program to greet a person
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Builds a new database given a local directory
    #[clap(short, long)]
    build_database: Option<String>,

    /// Directory to match against the database. If not specificed the current directory will be used
    #[clap(short, long)]
    match_dir: Option<String>,

    /// Makes it possible to remove paths in the db with the matching results. For example --filter_paths "/incoming" will remove any matching against the "incoming" directory. To filter more than one path use "/path1,/path2"
    #[clap(short, long, default_value = "")]
    filter_paths: String,
}

// Fetches info for a track/song
fn get_track_info(filename: &str) -> TrackInfo {
    let c_filename = std::ffi::CString::new(filename.as_bytes()).unwrap();
    let song_data = unsafe { hash_file(c_filename.as_ptr()) };

    // Calculate sha256 of the file
    let mut file = std::fs::File::open(&filename).unwrap();
    let mut file_data = Vec::new();
    file.read_to_end(&mut file_data).unwrap();
    let hash = sha2::Sha256::digest(&file_data);

    let mut track_info = TrackInfo {
        filename: filename.to_owned(),
        sha256_hash: format!("{:x}", hash),
        ..Default::default()
    };

    if !song_data.is_null() {
        let hash_id = unsafe { (*song_data).hash };
        let metadata = unsafe {
            SongMetadata {
                sample_names: get_string_cstr((*song_data).sample_names),
                //artist: get_string_cstr((*song_data).artist),
                //comments: get_string_cstr((*song_data).comments),
                //channel_count: (*song_data).channel_count,
            }
        };

        track_info.metadata = Some(metadata);
        track_info.pattern_hash = hash_id;

        unsafe { free_hash_data(song_data) };
    }

    track_info
}

// Updates the database with new entries
fn update_database(filepath: &str, conn: &Connection) {
    let files = get_files(filepath);

    println!("Hashing files");
    let pb = ProgressBar::new((files.len() * 2) as _);
    let data = Mutex::new(Vec::with_capacity(files.len()));

    files
        .par_iter()
        .enumerate()
        .for_each(|(_file_id, input_path)| {
            let track_info = get_track_info(input_path);

            pb.inc(1);

            {
                let mut tracks = data.lock().unwrap();
                tracks.push(track_info);
            }
        });

    let new_data = data.lock().unwrap();

    let mut hash_only_stmt = conn
        .prepare("INSERT INTO data (filehash, path) VALUES (:filehash, :path)")
        .unwrap();

    let mut stmt = conn
        .prepare("INSERT INTO data (filehash, pattern_hash, samples, path) VALUES (:filehash, :pattern_hash, :samples, :path)")
        .unwrap();

    conn.execute("BEGIN", []).unwrap();

    // Updating database
    for e in &*new_data {
        let filename = e.filename.replace(filepath, "");
        if let Some(metadata) = e.metadata.as_ref() {
            stmt.execute(&[
                (":filehash", &e.sha256_hash),
                (":pattern_hash", &e.pattern_hash.to_string()),
                (":samples", &metadata.sample_names),
                (":path", &filename),
            ])
            .unwrap();
        } else {
            hash_only_stmt
                .execute(&[(":filehash", &e.sha256_hash), (":path", &filename)])
                .unwrap();
        }

        pb.inc(1);
    }

    conn.execute("COMMIT", []).unwrap();
}
// tetsehou{
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

fn get_files_from_sha_hash(info: &TrackInfo, stmt: &mut Statement) -> Vec<String> {
    let rows = stmt
        .query_map(&[(":id", &info.sha256_hash)], |row| row.get(0))
        .unwrap();

    let mut names = Vec::new();
    for name_result in rows {
        names.push(name_result.unwrap());
    }

    names
}

fn get_files_from_pattern_hash(info: &TrackInfo, stmt: &mut Statement) -> Vec<String> {
    let rows = stmt
        .query_map(&[(":id", &info.pattern_hash.to_string())], |row| row.get(0))
        .unwrap();

    let mut names = Vec::new();
    for name_result in rows {
        names.push(name_result.unwrap());
    }

    names
}

fn filter_names<'a>(names: &[String], dir_filters: &str) -> Vec<String> {
    let mut output = Vec::new();

    if dir_filters.is_empty() {
        for f in names {
            output.push(f.to_owned());
        }

        return output;
    }

    let filter_paths = dir_filters.split(',');

    for t in filter_paths {
        for filename in names {
            if !filename.starts_with(t) {
                output.push(filename.to_owned());
                break;
            }
        }
    }

    output
}

fn match_dir_against_db(dir: &str, dir_filters: &str, db: &Connection) {
    let files = get_files(dir);

    let mut stmt = db
        .prepare("SELECT path FROM data where filehash = :id")
        .unwrap();

    let mut pattern_stmt = db
        .prepare("SELECT path FROM data where pattern_hash = :id")
        .unwrap();

    for filename in &files {
        let info = get_track_info(filename);
        println!("Matching {}", info.filename);

        let filenames = get_files_from_sha_hash(&info, &mut stmt);
        let filenames_pattern = get_files_from_pattern_hash(&info, &mut pattern_stmt);

        let filenames = filter_names(&filenames, dir_filters);
        let filenames_pattern = filter_names(&filenames_pattern, dir_filters);

        let mut found_entries = HashMap::new();

        for filename in &filenames {
            found_entries.insert(filename.to_owned(), (true, false));
        }

        for filename in &filenames_pattern {
            if let Some(v) = found_entries.get_mut(filename) {
                v.1 = true;
            } else {
                found_entries.insert(filename.to_owned(), (false, true));
            }
        }

        for found in &found_entries {
            let url = found.0.replace(" ", "%20");
            let url = format!("https://ftp.modland.com{}", url);
            if found.1 .0 && found.1 .1 {
                println!("Found match {} (hash) (pattern_hash)", url);
            } else if found.1 .0 && !found.1 .1 {
                println!("Found match {} (hash)", url);
            } else {
                println!("Found match {} (pattern_hash)", url);
            }
        }

        if found_entries.is_empty() {
            println!("No matches found!");
        }

        println!();
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    let conn;

    if let Some(db_path) = args.build_database.as_ref() {
        if std::path::Path::new("database.db").exists() {
            std::fs::remove_file("database.db").unwrap();
        }

        conn = Connection::open("database.db").unwrap();
        conn.execute(
            "
            CREATE TABLE data (
                path TEXT NOT_NUL,
                filehash TEXT NOT NULL,
                pattern_hash INTEGER,
                samples TEXT,
                PRIMARY KEY (path, filehash, pattern_hash)
            )",
            [], // empty list of parameters.
        )?;

        update_database(db_path, &conn);
    } else {
        conn = Connection::open("database.db")?;
    }

    if let Some(match_dir) = args.match_dir.as_ref() {
        match_dir_against_db(match_dir, &args.filter_paths, &conn);
    } else {
        match_dir_against_db(".", &args.filter_paths, &conn);
    }

    Ok(())
}

/*
"
*/

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
