pub mod m3u8;

use color_thief::ColorFormat;
use glob::glob;
use lofty::picture::{MimeType, PictureType};
use lofty::prelude::*;
use lofty::probe::Probe;
use lrc::Lyrics;
use m3u8::Playlist;
use mime_guess::{self, mime};
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use uuid::Uuid;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct LyricLine {
    pub start_time: i64,
    pub text: String,
}

#[derive(serde::Serialize, Debug)]
pub struct Cover {
    data: Vec<u8>,
    ext: String,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy)]
pub struct Color {
    r: u8,
    g: u8,
    b: u8,
}

impl Color {
    pub fn is_light_color(&self) -> bool {
        let luminance =
            0.2126 * (self.r as f64) + 0.7152 * (self.g as f64) + 0.0722 * (self.b as f64);
        let threshold = 180.0;

        luminance > threshold
    }
}

#[derive(serde::Serialize, serde::Deserialize, Default, Debug, Clone)]
pub struct Album {
    pub name: String,
    pub artist: String,
    pub tracks: Vec<Track>,
    pub year: Option<u32>,
    pub id: String,
}

impl Album {
    pub fn remove_track(&mut self, path: String) {
        self.tracks.retain(|x| x.file_path != path);
    }

    pub fn get_song(&self, id: &String) -> Option<Track> {
        for track in &self.tracks {
            if track.id == id.clone() {
                return Some(track.clone());
            }
        }

        None
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct Track {
    pub title: String,
    pub artists: Vec<String>,
    pub track: u32,
    pub album: String,
    pub album_artist: Option<String>,
    pub album_id: String,
    pub cover_ext: String,
    pub mime: String,
    pub album_year: Option<u32>,
    pub lyrics: Vec<LyricLine>,
    pub color: Option<Color>,
    pub is_light: Option<bool>,
    pub file_path: String,
    pub duration: u64,
    pub bitrate: u32,
    pub id: String,
    pub created_at: SystemTime,
}

impl Track {
    pub fn from_file(covers_dir: String, inode: PathBuf) -> Self {
        let tagged_file = Probe::open(&inode).unwrap().read().unwrap();
        let properties = tagged_file.properties();
        let bitrate = properties.audio_bitrate().unwrap_or(0);
        let duration = properties.duration();
        let mime = tagged_file.file_type();

        let default_tag = lofty::tag::Tag::new(lofty::tag::TagType::Id3v2);

        let tag = match tagged_file.primary_tag() {
            Some(primary_tag) => primary_tag,
            // If the "primary" tag doesn't exist, we just grab the
            // first tag we can find. Realistically, a tag reader would likely
            // iterate through the tags to find a suitable one.
            None => tagged_file.first_tag().unwrap_or(&default_tag),
        };

        let mut audio: Track = Track {
            file_path: inode.to_str().unwrap().to_string(),
            ..Default::default()
        };

        audio.mime = match mime {
            lofty::file::FileType::Aac => "audio/aac",
            lofty::file::FileType::Aiff => "audio/aiff",
            lofty::file::FileType::Ape => "audio/ape",
            lofty::file::FileType::Flac => "audio/flac",
            lofty::file::FileType::Mpeg => "audio/mpeg",
            lofty::file::FileType::Mp4 => "audio/mp4",
            lofty::file::FileType::Mpc => "audio/mpc",
            lofty::file::FileType::Opus => "audio/webm",
            lofty::file::FileType::Vorbis => "audio/webm",
            lofty::file::FileType::Speex => "audio/speex",
            lofty::file::FileType::Wav => "audio/wav",
            lofty::file::FileType::WavPack => "audio/wav",
            _ => "application/octet-stream",
        }
        .to_string();

        if let Ok(meta) = inode.metadata() {
            if let Ok(created_at) = meta.created() {
                audio.created_at = created_at;
            }
        };

        if let Some(year) = tag.year() {
            audio.album_year = Some(year);
        }

        if let Some(title) = tag.title() {
            audio.title = title.to_string();
        }
        if let Some(artists) = tag.get_string(&ItemKey::TrackArtist) {
            audio.artists = artists
                .split(';')
                .filter(|x| !x.is_empty())
                .map(|x| x.trim().to_string())
                .collect();
        };

        if let Some(album) = tag.album() {
            audio.album = album.to_string();
        }

        if let Some(album_artist) = tag.get_string(&ItemKey::OriginalArtist) {
            audio.album_artist = Some(album_artist.to_string());
        }

        if let Some(no) = tag.track() {
            audio.track = no;
        }

        let mut bytes = audio.album.as_bytes().to_vec();
        bytes.extend(
            audio
                .artists
                .first()
                .unwrap_or(&"@UNKNOWN@".to_string())
                .as_bytes(),
        );

        let digest = md5::compute(bytes);

        audio.album_id = format!("{digest:x}");

        let cover = tag.get_picture_type(PictureType::CoverFront);
        if let Some(cover) = cover {
            let mime = cover.mime_type().unwrap();
            let cover = Cover {
                data: cover.data().to_vec(),
                ext: match mime {
                    MimeType::Png => ".png".to_string(),
                    MimeType::Jpeg => ".jpeg".to_string(),
                    MimeType::Tiff => ".tiff".to_string(),
                    MimeType::Bmp => ".bmp".to_string(),
                    MimeType::Gif => ".gif".to_string(),
                    MimeType::Unknown(o) => format!(".{o}"),
                    _ => ".png".to_string(),
                },
            };

            let pathstr = format!("{covers_dir}/{digest:x}{}", cover.ext);
            let cover_path = std::path::Path::new(&pathstr);

            if !cover_path.exists() {
                check_dir(covers_dir);
                let mut f = fs::File::create(cover_path).unwrap();
                f.write_all(&cover.data).unwrap();
            }

            let img = image::open(cover_path).unwrap();
            let pixels = utils::get_image_buffer(img);

            let color = color_thief::get_palette(&pixels, ColorFormat::Rgb, 1, 2).unwrap();

            let color = Color {
                r: color[0].r,
                g: color[0].g,
                b: color[0].b,
            };

            audio.is_light = Some(color.is_light_color());
            audio.color = Some(color);
            audio.cover_ext = cover.ext;
        }

        audio.duration = duration.as_secs();
        audio.bitrate = bitrate;

        let lrc_path = inode.with_extension("lrc");
        if lrc_path.exists() {
            let mut f = fs::File::open(&lrc_path).unwrap();
            let mut buf = Vec::new();
            f.read_to_end(&mut buf).unwrap();
            let buf = String::from_utf8(buf);
            match buf {
                Ok(buf) => {
                    let buf = utils::remove_lyrics_tags(buf);
                    let lyrics = Lyrics::from_str(buf).unwrap();
                    let lines: Vec<LyricLine> = lyrics
                        .get_timed_lines()
                        .iter()
                        .map(|(time, content)| LyricLine {
                            start_time: time.get_timestamp(),
                            text: content.to_string(),
                        })
                        .collect();

                    audio.lyrics = lines;
                }
                Err(e) => {
                    eprintln!("{e}");
                }
            }
        }

        audio
    }
}

impl Default for Track {
    fn default() -> Self {
        Self {
            title: "@UNKNOWN@".to_string(),
            artists: vec![],
            track: 0,
            album: "@UNKNOWN@".to_string(),
            album_artist: None,
            album_id: String::new(),
            album_year: None,
            lyrics: vec![],
            cover_ext: ".png".to_string(),
            mime: "audio/mp3".to_string(),
            color: None,
            is_light: None,
            file_path: String::new(),
            bitrate: 0,
            duration: 0,
            id: format!("{:x}", Uuid::new_v4().as_u128()),
            created_at: SystemTime::UNIX_EPOCH,
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Default, Debug, Clone)]
pub struct Songs {
    pub audios: Vec<Track>,
}

#[derive(serde::Serialize, serde::Deserialize, Default, Debug, Clone)]
pub struct Media {
    pub albums: Vec<Album>,
    pub playlists: Vec<Playlist>,
}

impl Media {
    pub fn new(songs: Songs) -> Self {
        Self {
            albums: songs.get_albums(),
            playlists: vec![],
        }
    }

    pub fn add_song(&mut self, song: Track) {
        let mut inserted = false;
        for album in &mut self.albums {
            if song.album_id == album.id {
                album.tracks.push(song.clone());
                inserted = true;
                break;
            }
        }
        if !inserted {
            let albums = Songs { audios: vec![song] }.get_albums();
            self.albums.extend(albums);
        }
    }

    pub fn add_media(&mut self, path: PathBuf, covers_dir: String) {
        let ext = path.extension().unwrap().to_str().unwrap();
        if ext == "m3u8" {
            self.add_playlist(m3u8::M3U8::parse(covers_dir, path));
        } else {
            self.add_song(Track::from_file(covers_dir.clone(), path));
        }
    }

    pub fn remove_media(&mut self, path: PathBuf) {
        let ext = path.extension().unwrap().to_str().unwrap();
        if ext == "m3u8" {
            self.remove_playlist(path);
        } else {
            self.remove_song(path);
        }
    }

    #[inline]
    pub fn add_playlist(&mut self, playlist: Playlist) {
        self.playlists.push(playlist);
    }

    #[inline]
    pub fn remove_playlist(&mut self, path: PathBuf) {
        self.playlists
            .retain(|x| x.path != format!("{}", path.display()));
    }

    pub fn remove_song(&mut self, path: PathBuf) {
        let string_path = format!("{}", path.display());
        for album in &mut self.albums {
            album.remove_track(string_path.clone());
        }

        self.albums.retain(|x| !x.tracks.is_empty());
    }

    pub fn get_album(&self, id: &String) -> Option<Album> {
        for album in self.albums.iter() {
            if album.id == id.clone() {
                return Some(Album {
                    name: album.name.clone(),
                    artist: album.artist.clone(),
                    tracks: album.tracks.clone(),
                    year: album.year,
                    id: album.id.clone(),
                });
            }
        }

        None
    }

    pub fn get_song(&self, id: &String) -> Option<Track> {
        for album in &self.albums {
            let res = album.get_song(id);
            if res.is_some() {
                return res;
            }
        }

        for playlist in &self.playlists {
            let res = playlist.get_song(id);
            if res.is_some() {
                return res;
            }
        }

        None
    }
}

impl Songs {
    pub fn get_albums(self) -> Vec<Album> {
        let mut albums = vec![];
        let mut album_map: HashMap<String, Vec<Track>> = HashMap::new();
        for audio in self.audios {
            album_map
                .entry(audio.album_id.clone())
                .or_default()
                .push(audio);
        }

        for (k, v) in album_map {
            albums.push(Album {
                name: v[0].album.clone(),
                artist: v[0].album_artist.clone().unwrap_or(String::from(
                    v[0].artists
                        .clone()
                        .first()
                        .unwrap_or(&"@UNKNOWN@".to_string()),
                )),
                year: v[0].album_year,
                tracks: v,
                id: k,
            });
        }

        albums
    }
}

pub fn check_dir(dir: String) {
    let dir = Path::new(&dir);
    if !dir.exists() {
        fs::DirBuilder::new().recursive(true).create(dir).unwrap();
    }
}

pub mod utils {
    use std::{
        io::{Read, Write},
        path::PathBuf,
        str::FromStr,
    };

    use crate::glob;

    pub fn get_image_buffer(img: image::DynamicImage) -> Vec<u8> {
        match img {
            image::DynamicImage::ImageRgb8(buffer) => buffer.to_vec(),
            _ => unreachable!(),
        }
    }

    pub fn remove_lyrics_tags(buf: String) -> String {
        let strings: Vec<String> = buf
            .lines()
            .filter(|x| !x.is_empty())
            .filter(|x| {
                let mut chars = x.chars();
                let openparen = chars.next().unwrap();
                let nextdata = chars.next().unwrap();
                openparen == '[' && nextdata.is_ascii_digit()
            })
            .map(|x| x.to_string())
            .collect();

        strings.join("\n")
    }

    pub fn get_audio_files() -> Vec<PathBuf> {
        let mut files = vec![];
        if let Some(audio_dir) = dirs::audio_dir() {
            if let Ok(paths) = glob(&format!("{}/**/*", audio_dir.display())) {
                for inode in paths.flatten() {
                    if inode.is_file() {
                        let guess =
                            mime_guess::from_path(&inode).first_or("text/plain".parse().unwrap());
                        if guess.type_() == super::mime::AUDIO {
                            files.push(inode);
                        }
                    }
                }
            }
        }
        files
    }

    pub fn cache_audio_files(cache_path: &std::path::Path) {
        let files: Vec<String> = get_audio_files()
            .iter()
            .map(|x| format!("{}", x.display()))
            .collect();
        let data = files.join("\n");
        let mut f = std::fs::File::create(cache_path).unwrap();
        let _ = f.write_all(data.as_bytes());
    }

    pub fn read_cahe_audio_files(cache_path: &std::path::Path) -> Vec<PathBuf> {
        let mut buf = String::new();
        if cache_path.exists() {
            let mut f = std::fs::File::open(cache_path).unwrap();
            let _ = f.read_to_string(&mut buf);
        }

        buf.lines().map(|x| PathBuf::from_str(x).unwrap()).collect()
    }

    pub fn music_dir_md5() -> String {
        let mut files = vec![];
        if let Some(music_dir) = dirs::audio_dir() {
            if let Ok(paths) = glob(&format!("{}/**/*", music_dir.display())) {
                for p in paths {
                    if p.is_ok() {
                        let path_buf = p.unwrap();
                        files.extend(path_buf.to_str().unwrap().as_bytes());
                    }
                }

                let digest = md5::compute(files);

                format!("{digest:x}")
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    }
}

impl Songs {
    pub fn new(cache_dir: PathBuf, audio_files: Vec<PathBuf>) -> Self {
        check_dir(format!("{}/covers", cache_dir.display()));
        let covers_dir = format!("{}/covers", cache_dir.display());
        let mut audios: Vec<Track> = vec![];

        for audio_file in audio_files {
            audios.push(Track::from_file(covers_dir.clone(), audio_file))
        }

        Self { audios }
    }
}