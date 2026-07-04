// build.rs
use std::fs;
use std::path::Path;

fn main() {
    // Re-run this build script if albums.json changes
    println!("cargo:rerun-if-changed=src/public/albums.json");

    let albums_data = fs::read_to_string("src/public/albums.json").unwrap_or_else(|_| String::from("[]"));
    let albums: serde_json::Value = serde_json::from_str(&albums_data).unwrap_or(serde_json::Value::Null);

    let mut generated_code = String::new();
    generated_code.push_str("pub fn get_embedded_audio(album: &str, track_num: u32, track_name: &str) -> Option<&'static [u8]> {\n");
    generated_code.push_str("    let key = format!(\"{}_{}_{}\", album, track_num, track_name);\n");
    generated_code.push_str("    match key.as_str() {\n");

    if let Some(albums_list) = albums.as_array() {
        for album in albums_list {
            let album_title = album["album_title"].as_str().unwrap_or("");
            let mut clean_album = album_title.to_string();
            for c in &['<', '>', ':', '"', '/', '\\', '|', '?', '*'] {
                clean_album = clean_album.replace(*c, "");
            }

            if let Some(tracks) = album["tracks"].as_array() {
                for track in tracks {
                    let track_num = track["track_number"].as_u64().unwrap_or(0);
                    let track_name = track["track_name"].as_str().unwrap_or("");
                    let mut clean_track = format!("{:02} - {}", track_num, track_name);
                    for c in &['<', '>', ':', '"', '/', '\\', '|', '?', '*'] {
                        clean_track = clean_track.replace(*c, "");
                    }

                    let relative_path = format!("downloads/{}/{}.mp3", clean_album, clean_track);
                    let absolute_path = Path::new(&relative_path);
                    if absolute_path.exists() {
                        generated_code.push_str(&format!(
                            "        {:?} => Some(include_bytes!(\"../{}\")),\n",
                            format!("{}_{}_{}", album_title, track_num, track_name),
                            relative_path
                        ));
                    }
                }
            }
        }
    }

    generated_code.push_str("        _ => None,\n");
    generated_code.push_str("    }\n");
    generated_code.push_str("}\n");

    fs::write("src/embedded_data.rs", generated_code).unwrap();
}
