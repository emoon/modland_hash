use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::sync::Mutex;
use walkdir::WalkDir;
use std::io::BufReader;
use std::os::raw::c_char;

fn get_files(path: &str) -> Vec<String> {
    let files: Vec<String> = WalkDir::new(path)
        .into_iter()
        .filter_map(|e| {
            let file = e.unwrap();
            let metadata = file.metadata().unwrap();

            if let Some(filename) = file.path().to_str() {
                if metadata.is_file()
                    && !filename.ends_with(".listing")
                    && !filename.ends_with(".psflib")
                    && !filename.ends_with(".ssflib")
                    && !filename.ends_with(".mdx")
                    && !filename.ends_with(".MDX")
                    && !filename.ends_with(".pdx")
                    && !filename.ends_with(".PDX")
                {
                    return Some(file);
                }
            }

            None
        })
        .map(|entry| entry.into_path().to_str().unwrap().to_string())
        .collect();
    files
}

#[derive(Hash, Eq, PartialEq, Debug)]
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

extern {
    fn hash_file(filename: *const i8) -> *const CData;
    fn free_hash_data(data: *const CData);
}

#[derive(Default)]
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

fn main() {
    //println!("getting files...");
    let files = get_files("/home/emoon/Music/ftp.modland.com/pub/modules");

    //let mut hash_map = Hashmap::new(

    let data = Mutex::new(HashMap::<u64, Vec<SongMetadata>>::new());

    //let total_count = files.len();

    files
        .par_iter()
        //.iter()
        .enumerate()
        .for_each(|(_file_id, input_path)| {
            //let mut pattern_data = Vec::with_capacity(40 * 1024);
            let song_data;

            {
                let c_filename = std::ffi::CString::new(input_path.as_bytes()).unwrap();
                song_data = unsafe { hash_file(c_filename.as_ptr()) };
            }

            //println!("processing ({} / {}) {}", file_id, total_count, input_path);
            //let song = mod_player::read_mod_file(input_path);
            /*
            let file = std::fs::File::open(input_path).unwrap();
            let mut reader = BufReader::new(file);
            //let mut file = fs::File::open(&input_path).unwrap();
            let mut sha256 = Sha256::new();
            sha256.update(&pattern_data);
            //io::copy(&mut pattern_data, &mut sha256).unwrap();
            let hash = sha256.finalize();

            let hash = Hash {
                v0: get_u64_from_u8(&hash[0..7]),
                v1: get_u64_from_u8(&hash[8..15]),
                v2: get_u64_from_u8(&hash[16..23]),
                v3: get_u64_from_u8(&hash[24..31]),
            };
            */

            if song_data != std::ptr::null() {
                let mut map = data.lock().unwrap();
                let hash_id = unsafe { (*song_data).hash };
                let metadata = unsafe { SongMetadata {
                    filename: input_path.to_owned(),
                    sample_names: get_string_cstr((*song_data).sample_names),
                    artist: get_string_cstr((*song_data).artist),
                    comments: get_string_cstr((*song_data).comments),
                    channel_count: (*song_data).channel_count,
                } };

                if let Some(x) = map.get_mut(&hash_id) {
                    x.push(metadata);
                } else {
                    map.insert(hash_id, vec![metadata]);
                }

                unsafe { free_hash_data(song_data) };
            }
        });

    let mut output = String::with_capacity(10 * 1024 * 1024);
    let mut count = 0;
    let map = data.lock().unwrap();

    output.push_str(HTML_HEADER);

    let mut dupe_array = Vec::new();

    for (_key, val) in map.iter() {
        if val.len() > 1 {
            dupe_array.push(val);
        }
    }

    dupe_array.sort_by(|a, b| a[0].filename.cmp(&b[0].filename));

    for val in dupe_array {
        output.push_str(&format!("<h1 id='dupe_{}'>Dupe {}</h1><hr>\n", count, count));
        for t in val {
            let name = &t.filename[18..];
            output.push_str(&format!("<a href=\"https://{}\">{}</a>\n", name, name));
            output.push_str("</code><pre>");
            output.push_str("<pre><code>");
            output.push_str(&t.sample_names.trim_end_matches('\n'));
            output.push_str("</code><pre>");
        }
        count += 1;
    }

    println!("{}", output);
}

static HTML_HEADER: &str =
"<html xmlns=\"http://www.w3.org/1999/xhtml\" xml:lang=\"en\" lang=\"en\">
<head>
	<style type=\"text/css\" media=\"screen\">
		body {
			line-height: 140%;
			margin: 50px;
			width: 650px;
		}
		code {font-size: 120%;}
		pre code {
			background-color: #eee;
			border: 1px solid #999;
			display: block;
			padding: 20px;
		}
	</style>
</head>

";

