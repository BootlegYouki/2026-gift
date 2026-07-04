#![allow(dead_code)]
use std::collections::VecDeque;
use std::error::Error;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use rand::seq::SliceRandom;
use rodio::Source;

/// Shared ring buffer of recent decoded sample amplitudes, fed live by `AmpTap`
/// as the sink plays them, and drained by the now-playing waveform to draw an
/// actual oscilloscope trace of what's currently coming out of the speakers.
type AmpBuf = Arc<Mutex<VecDeque<f32>>>;
const AMP_BUF_CAP: usize = 44_100 * 2; // ~1s of interleaved stereo samples

/// Wraps a rodio `Source`, tapping each sample's amplitude into a shared
/// buffer as it's consumed by the sink, without altering playback.
struct AmpTap<S> {
    inner: S,
    buf: AmpBuf,
}

impl<S> Iterator for AmpTap<S>
where
    S: Source,
    S::Item: rodio::Sample,
    f32: rodio::cpal::FromSample<S::Item>,
{
    type Item = S::Item;
    fn next(&mut self) -> Option<Self::Item> {
        let sample = self.inner.next()?;
        let amp: f32 = rodio::cpal::Sample::from_sample(sample);
        if let Ok(mut buf) = self.buf.lock() {
            if buf.len() >= AMP_BUF_CAP {
                buf.pop_front();
            }
            buf.push_back(amp);
        }
        Some(sample)
    }
}

impl<S> Source for AmpTap<S>
where
    S: Source,
    S::Item: rodio::Sample,
    f32: rodio::cpal::FromSample<S::Item>,
{
    fn current_frame_len(&self) -> Option<usize> {
        self.inner.current_frame_len()
    }
    fn channels(&self) -> u16 {
        self.inner.channels()
    }
    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
    fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        self.inner.try_seek(pos)
    }
}

use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Paragraph, Block, BorderType, Borders, Padding, Row, Table, Cell, Gauge, TableState},
    Terminal,
};
use crossterm::event::{self, Event, KeyCode, MouseEventKind, MouseButton};
use rodio;
use mp3_duration;

#[derive(Clone, Debug)]
pub struct Song {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub date_added: String,
    pub duration: String,
    pub duration_seconds: u32,
}

#[derive(Clone, Debug)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub songs: Vec<Song>,
}

enum ActivePanel {
    Sidebar,
    MainList,
}

use serde::Deserialize;

#[derive(Deserialize)]
struct JsonTrack {
    track_number: u32,
    track_name: String,
    date_added: String,
}

#[derive(Deserialize)]
struct JsonAlbum {
    album_title: String,
    release_year: u32,
    total_tracks: u32,
    tracks: Vec<JsonTrack>,
}

fn play_current_song(sink: &Option<rodio::Sink>, playlist: &Playlist, song_idx: usize, amp_buf: &AmpBuf) {
    if let Some(ref s) = sink {
        s.stop();
        if let Ok(mut buf) = amp_buf.lock() {
            buf.clear();
        }
        if song_idx < playlist.songs.len() {
            let song = &playlist.songs[song_idx];
            if let Some(bytes) = crate::embedded_data::get_embedded_audio(&playlist.name, (song_idx + 1) as u32, &song.title) {
                let cursor = std::io::Cursor::new(bytes);
                if let Ok(source) = rodio::Decoder::new(cursor) {
                    s.append(AmpTap { inner: source, buf: amp_buf.clone() });
                    s.play();
                }
            }
        }
    }
}

// How many tracks the "Next in Queue" panel wants to show ahead at once.
const QUEUE_LOOKAHEAD: usize = 50;

// Top the upcoming-play queue back up to QUEUE_LOOKAHEAD with fresh shuffled
// rounds (excluding the currently playing song) so the display never visibly
// runs dry — it keeps appending more shuffles, forever.
fn ensure_queue(queue: &mut VecDeque<usize>, current: usize, len: usize) {
    if len <= 1 {
        return;
    }
    while queue.len() < QUEUE_LOOKAHEAD {
        let mut order: Vec<usize> = (0..len).filter(|&i| i != current).collect();
        order.shuffle(&mut rand::thread_rng());
        queue.extend(order);
    }
}

fn pop_next_song(queue: &mut VecDeque<usize>, current: usize, len: usize) -> usize {
    ensure_queue(queue, current, len);
    queue.pop_front().unwrap_or(current)
}

fn seek_current_song(sink: &Option<rodio::Sink>, playlist: &Playlist, song_idx: usize, seek_secs: u32, amp_buf: &AmpBuf) {
    if let Some(ref s) = sink {
        // If the sink already has the song loaded, use try_seek for instant seeking
        if !s.empty() {
            let _ = s.try_seek(std::time::Duration::from_secs(seek_secs as u64));
            return;
        }
        // Fallback: load song first then seek
        s.stop();
        if let Ok(mut buf) = amp_buf.lock() {
            buf.clear();
        }
        if song_idx < playlist.songs.len() {
            let song = &playlist.songs[song_idx];
            if let Some(bytes) = crate::embedded_data::get_embedded_audio(&playlist.name, (song_idx + 1) as u32, &song.title) {
                let cursor = std::io::Cursor::new(bytes);
                if let Ok(source) = rodio::Decoder::new(cursor) {
                    s.append(AmpTap { inner: source, buf: amp_buf.clone() });
                    s.play();
                    let _ = s.try_seek(std::time::Duration::from_secs(seek_secs as u64));
                }
            }
        }
    }
}

fn format_date_added(raw_date: &str) -> String {
    if raw_date.len() < 10 {
        return raw_date.to_string();
    }
    let date_part = &raw_date[0..10]; // YYYY-MM-DD
    let parts: Vec<&str> = date_part.split('-').collect();
    if parts.len() != 3 {
        return date_part.to_string();
    }
    let year = parts[0];
    let month_str = parts[1];
    let day_str = parts[2];
    
    let month_name = match month_str {
        "01" => "Jan",
        "02" => "Feb",
        "03" => "Mar",
        "04" => "Apr",
        "05" => "May",
        "06" => "Jun",
        "07" => "Jul",
        "08" => "Aug",
        "09" => "Sep",
        "10" => "Oct",
        "11" => "Nov",
        "12" => "Dec",
        _ => month_str,
    };
    
    let day = day_str.trim_start_matches('0');
    let day = if day.is_empty() { "0" } else { day };
    
    format!("{} {}, {}", month_name, day, year)
}

fn marquee_text(text: &str, width: usize, offset: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= width {
        return text.to_string();
    }
    
    let scroll_gap = 10;
    let cycle = char_count + scroll_gap;
    let start = offset % cycle;
    
    let repeated = format!("{}          {}", text, text);
    let chars: Vec<char> = repeated.chars().collect();
    if start + width <= chars.len() {
        chars[start..start + width].iter().collect()
    } else {
        chars[start..].iter().collect()
    }
}

pub fn run_secret_screen(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> Result<(), Box<dyn Error>> {
    // Load playlists embedded directly inside the binary
    let albums_data = include_str!("public/albums.json");
    let json_albums: Vec<JsonAlbum> = serde_json::from_str(albums_data).unwrap_or_default();
    
    let mut playlists = Vec::new();
    let mut global_song_id = 1;
    for album in json_albums {
        let mut songs = Vec::new();
        for track in album.tracks {
            let duration_seconds = if let Some(bytes) = crate::embedded_data::get_embedded_audio(&album.album_title, track.track_number, &track.track_name) {
                let mut cursor = std::io::Cursor::new(bytes);
                match mp3_duration::from_read(&mut cursor) {
                    Ok(duration) => duration.as_secs() as u32,
                    Err(_) => 180,
                }
            } else {
                180 + ((track.track_name.len() * 7 + track.track_number as usize * 13) % 90) as u32
            };
            let mins = duration_seconds / 60;
            let secs = duration_seconds % 60;
            let duration = format!("{}:{:02}", mins, secs);

            let date = format_date_added(&track.date_added);

            songs.push(Song {
                id: global_song_id.to_string(),
                title: track.track_name,
                artist: String::from("BTS"),
                album: album.album_title.clone(),
                date_added: date,
                duration,
                duration_seconds,
            });
            global_song_id += 1;
        }
        playlists.push(Playlist {
            id: album.album_title.to_lowercase().replace(" ", "_"),
            name: album.album_title,
            songs,
        });
    }

    if playlists.is_empty() {
        // Fallback to avoid crash if file is missing/empty
        playlists.push(Playlist {
            id: String::from("empty"),
            name: String::from("No Albums Loaded"),
            songs: vec![],
        });
    }

    let mut active_playlist_idx = 0;
    let mut selected_playlist_idx = 0;
    let mut selected_song_idx = 0;
    let mut active_panel = ActivePanel::Sidebar;

    // Simulated local playback state
    let mut playing_song_idx = 0;
    let mut playing_playlist_idx = 0;
    let mut upcoming_queue: VecDeque<usize> = VecDeque::new();
    let amp_buf: AmpBuf = Arc::new(Mutex::new(VecDeque::new()));
    let mut is_playing = false;
    let mut play_seconds = 0;
    let mut last_tick = Instant::now();
    let tick_rate = Duration::from_millis(100);
    let mut volume: u32 = 50;
 
    // Initialize rodio audio stream and sink
    let (_stream, stream_handle) = match rodio::OutputStream::try_default() {
        Ok(val) => (Some(val.0), Some(val.1)),
        Err(_) => (None, None),
    };
    
    let sink = if let Some(ref handle) = stream_handle {
        match rodio::Sink::try_new(handle) {
            Ok(s) => {
                s.set_volume(volume as f32 / 100.0);
                Some(s)
            }
            Err(_) => None,
        }
    } else {
        None
    };

    // Sidebar scrolling viewport offset
    let mut sidebar_scroll = 0;
    
    // Main playlist Table scroll state
    let mut table_state = TableState::default();
    table_state.select(Some(0));

    let mut marquee_timer = Instant::now();
    let mut marquee_offset = 0;

    // Keep track of Rect locations for mouse clicks
    let mut sidebar_rect = Rect::default();
    let mut sidebar_inner = Rect::default();
    let mut main_rect = Rect::default();
    let mut main_inner = Rect::default();
    let mut player_rect = Rect::default();
    let mut player_inner = Rect::default();
    let mut list_start = 0;
    let mut player_rows_save = vec![Rect::default(); 3];
    let mut progress_bar_rect_save = Rect::default();
    let mut player_top_cols_save = vec![Rect::default(); 3];
    
    let mut mouse_pos: Option<(u16, u16)> = None;

    loop {
        if marquee_timer.elapsed() >= Duration::from_millis(150) {
            marquee_timer = Instant::now();
            marquee_offset += 1;
        }

        let current_playlist = &playlists[active_playlist_idx];
        let playing_playlist = &playlists[playing_playlist_idx];
        let playing_song = &playing_playlist.songs[playing_song_idx];

        // Playback progress simulation
        if last_tick.elapsed() >= Duration::from_secs(1) {
            last_tick = Instant::now();
            
            let mut song_finished = false;
            if let Some(ref s) = sink {
                if !s.empty() && is_playing {
                    play_seconds += 1;
                } else if s.empty() && is_playing {
                    song_finished = true;
                }
            } else {
                if is_playing {
                    play_seconds += 1;
                    if play_seconds >= playing_song.duration_seconds {
                        song_finished = true;
                    }
                }
            }

            if song_finished {
                play_seconds = 0;
                playing_song_idx = pop_next_song(&mut upcoming_queue, playing_song_idx, playing_playlist.songs.len());
                play_current_song(&sink, &playlists[playing_playlist_idx], playing_song_idx, &amp_buf);
            }
        }

        // Auto-scroll the library sidebar to keep selected playlist in view
        let selected_sidebar_line = 3 + (selected_playlist_idx * 2) as u16;
        let viewport_height = sidebar_inner.height;
        if viewport_height > 0 {
            if selected_sidebar_line < sidebar_scroll {
                sidebar_scroll = selected_sidebar_line;
            } else if selected_sidebar_line >= sidebar_scroll + viewport_height {
                sidebar_scroll = selected_sidebar_line - viewport_height + 1;
            }
        }

        // Draw TUI
        terminal.draw(|f| {
            let area = f.area();

            // Main layout: Vertical split between body and playing bar
            let main_layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(0),
                    Constraint::Length(8),
                ])
                .split(area);

            player_rect = main_layout[1];

            // Body layout: Horizontal split — sidebar | main | now playing.
            // Before any song has ever loaded, main expands to fill the space;
            // once a song loads it stays reserved (even while paused).
            let has_loaded_song = sink.as_ref().map_or(false, |s| !s.empty()) || is_playing;
            let body_layout = if has_loaded_song {
                Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Percentage(18),
                        Constraint::Percentage(57),
                        Constraint::Percentage(25),
                    ])
                    .split(main_layout[0])
            } else {
                Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Percentage(18),
                        Constraint::Percentage(82),
                        Constraint::Percentage(0),
                    ])
                    .split(main_layout[0])
            };

            sidebar_rect = body_layout[0];
            main_rect = body_layout[1];
            let now_playing_rect = body_layout[2];

            // ------------------ 1. Library Sidebar ------------------
            let sidebar_hovered = mouse_pos.map_or(false, |(mx, my)| contains_point(sidebar_rect, mx, my));
            let sidebar_border_style = if sidebar_hovered {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let sidebar_block = Block::default()
                .title(" Library ")
                .borders(Borders::ALL)
                .border_type(BorderType::Plain)
                .border_style(sidebar_border_style)
                .padding(Padding::uniform(1));

            sidebar_inner = sidebar_block.inner(sidebar_rect);

            // Render playlist list content
            let mut playlist_lines = vec![
                Line::from(vec![Span::styled("Bibi's Library", Style::default().add_modifier(Modifier::BOLD).fg(Color::White))]),
                Line::from(vec![Span::styled("Repackages and Anthologies", Style::default().fg(Color::DarkGray))]),
                Line::from(""),
            ];

            for (idx, pl) in playlists.iter().enumerate() {
                let symbol = "  ";
                let style = if idx == selected_playlist_idx && matches!(active_panel, ActivePanel::Sidebar) {
                    Style::default().fg(Color::Black).bg(Color::Green)
                } else if idx == active_playlist_idx {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::White)
                };

                playlist_lines.push(Line::styled(
                    format!("{}{}", symbol, pl.name),
                    style,
                ));
                playlist_lines.push(Line::from("")); // Spacer line between playlists
            }

            f.render_widget(sidebar_block, sidebar_rect);
            f.render_widget(Paragraph::new(playlist_lines).scroll((sidebar_scroll, 0)), sidebar_inner);

            // ------------------ 2. Main Content ------------------
            let main_hovered = mouse_pos.map_or(false, |(mx, my)| contains_point(main_rect, mx, my));
            let main_border_style = if main_hovered {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let main_block = Block::default()
                .title(format!(" Playlist: {} ", current_playlist.name))
                .borders(Borders::ALL)
                .border_type(BorderType::Plain)
                .border_style(main_border_style)
                .padding(Padding::uniform(1));

            main_inner = main_block.inner(main_rect);

            // Returns the signature color for each album title
            let album_color = |name: &str| -> Color {
                if name.contains("ARIRANG") {
                    Color::Rgb(230, 57, 70)
                } else if name.contains("Proof") {
                    Color::Rgb(192, 192, 192)
                } else if name.contains("Love Yourself") {
                    Color::Rgb(224, 176, 255)
                } else if name.contains("Most Beautiful") {
                    Color::Rgb(135, 206, 235)
                } else {
                    Color::Green
                }
            };

            // Playlist Header Info
            let mut main_content_lines = vec![
                Line::from(""), // Vertical spacing gap
            ];

            let clean_name: String = current_playlist.name.chars()
                .filter(|c| c.is_ascii())
                .collect();
            let display_name = clean_name.trim();

            if let Some(shadow_lines) = crate::text_renderer::ShadowTextRenderer::render(display_name) {
                let total_len = shadow_lines[0].chars().count();
                let visible_width = (main_inner.width as usize).saturating_sub(2).max(10);
                
                if total_len > visible_width {
                    let scroll_gap = 12;
                    let cycle = total_len + scroll_gap;
                    let start_col = marquee_offset % cycle;
                    
                    for i in 0..6 {
                        let mut repeat_str = String::new();
                        while repeat_str.chars().count() < start_col + visible_width {
                            repeat_str.push_str(&shadow_lines[i]);
                            repeat_str.push_str("            ");
                        }
                        let sliced: String = repeat_str.chars().skip(start_col).take(visible_width).collect();
                        main_content_lines.push(Line::styled(sliced, Style::default().fg(album_color(display_name)).add_modifier(Modifier::BOLD)));
                    }
                } else {
                    for line in shadow_lines {
                        main_content_lines.push(Line::styled(line, Style::default().fg(album_color(display_name)).add_modifier(Modifier::BOLD)));
                    }
                }
            } else {
                main_content_lines.push(Line::from(vec![Span::styled(&current_playlist.name, Style::default().fg(album_color(display_name)).add_modifier(Modifier::BOLD))]));
            }

            let total_dur = current_playlist.songs.iter().map(|s| s.duration_seconds).sum();
            main_content_lines.push(Line::from(vec![Span::styled(
                format!("• {} songs, {}", current_playlist.songs.len(), format_duration(total_dur)),
                Style::default().fg(Color::DarkGray),
            )]));
            main_content_lines.push(Line::from(""));

            // Render header lines first
            let header_height = main_content_lines.len() as u16;
            
            // Build Table for song list
            let header_row = Row::new(vec![
                Cell::from("#").fg(Color::DarkGray),
                Cell::from("Title").fg(Color::DarkGray),
                Cell::from("Album").fg(Color::DarkGray),
                Cell::from("Date added").fg(Color::DarkGray),
                Cell::from("[┘]").fg(Color::DarkGray),
            ]).bottom_margin(1);

            let mut rows = Vec::new();
            for (idx, song) in current_playlist.songs.iter().enumerate() {
                let song_number = (idx + 1).to_string();
                let is_current = playing_playlist_idx == active_playlist_idx && playing_song_idx == idx;
                let is_selected = idx == selected_song_idx && matches!(active_panel, ActivePanel::MainList);
                
                let row_style = if is_selected {
                    Style::default().bg(Color::Green)
                } else {
                    Style::default()
                };

                let index_style = if is_selected {
                    Style::default().fg(Color::Black)
                } else if is_current {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::White)
                };

                let title_style = if is_selected {
                    Style::default().fg(Color::Black).add_modifier(Modifier::BOLD)
                } else if is_current {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::White)
                };

                let artist_style = if is_selected {
                    Style::default().fg(Color::Rgb(30, 80, 30))
                } else {
                    Style::default().fg(Color::DarkGray)
                };

                let album_style = if is_selected {
                    Style::default().fg(Color::Black)
                } else {
                    Style::default().fg(Color::White)
                };

                let date_style = if is_selected {
                    Style::default().fg(Color::Black)
                } else {
                    Style::default().fg(Color::White)
                };

                let duration_style = if is_selected {
                    Style::default().fg(Color::Black)
                } else if is_current {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::White)
                };

                let album_width = ((main_inner.width as usize * 35) / 100).saturating_sub(1).max(5);
                let title_width = ((main_inner.width as usize * 35) / 100).saturating_sub(1).max(10);
 
                let display_title = marquee_text(&song.title, title_width, marquee_offset);
                let display_album = marquee_text(&song.album, album_width, marquee_offset);

                rows.push(Row::new(vec![
                    Cell::from(song_number).style(index_style),
                    Cell::from(vec![
                        Line::styled(display_title, title_style),
                        Line::styled(&song.artist, artist_style),
                    ]),
                    Cell::from(display_album).style(album_style),
                    Cell::from(song.date_added.as_str()).style(date_style),
                    Cell::from(song.duration.as_str()).style(duration_style),
                ]).style(row_style).height(2));

                rows.push(Row::new(vec![
                    Cell::from(""),
                    Cell::from(""),
                    Cell::from(""),
                    Cell::from(""),
                    Cell::from(""),
                ]).height(1));
            }

            let table_widths = [
                Constraint::Length(4),
                Constraint::Percentage(35),
                Constraint::Percentage(35),
                Constraint::Percentage(15),
                Constraint::Length(6),
            ];

            let song_table = Table::new(rows, table_widths)
                .header(header_row)
                .block(Block::default());

            // Draw everything in the main block
            let main_split = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(header_height),
                    Constraint::Min(0),
                ])
                .split(main_inner);

            list_start = main_split[1].y + 2;

            f.render_widget(main_block, main_rect);
            f.render_widget(Paragraph::new(main_content_lines), main_split[0]);
            f.render_stateful_widget(song_table, main_split[1], &mut table_state);

            // ------------------ 3. Now Playing + Queue Panel ------------------
            // Stays visible while paused — only hidden before anything has ever loaded.
            if has_loaded_song && now_playing_rect.width > 2 {
                let np_color = album_color(&playing_song.album);

                // Split the right column: top = Now Playing, bottom = Next in Queue
                let right_split = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Percentage(70),
                        Constraint::Percentage(30),
                    ])
                    .split(now_playing_rect);

                let np_rect    = right_split[0];
                let queue_rect = right_split[1];

                // ---- Now Playing block ----
                let np_block = Block::default()
                    .title(format!(" {} ", playing_song.album))
                    .borders(Borders::ALL)
                    .border_type(BorderType::Plain)
                    .border_style(Style::default().fg(np_color))
                    .padding(Padding::uniform(1));

                let np_inner = np_block.inner(np_rect);
                f.render_widget(np_block, np_rect);

                // ── Art sizing ────────────────────────────────────────────────
                // Text rows below art: gap(1)+title(1)+artist(1)+album(1)+gap(1)+waveform(1) = 6
                let text_rows = 6u16;
                // How many terminal rows we can give to art without overflowing
                let art_h_terminal = np_inner.height.saturating_sub(text_rows).max(4) as usize;
                // With ▀ half-blocks, chars are ~2:1 tall, so art_w = art_h × 2 → visually square
                let art_w_cols = (art_h_terminal * 2).min(np_inner.width as usize);
                // Centre the art inside the panel
                let art_left_pad = (np_inner.width as usize).saturating_sub(art_w_cols) / 2;

                // Build sections using the computed art height
                let np_sections = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(art_h_terminal as u16), // [0] art
                        Constraint::Length(1),                       // [1] gap
                        Constraint::Length(1),                       // [2] title
                        Constraint::Length(1),                       // [3] artist
                        Constraint::Length(1),                       // [4] album
                        Constraint::Length(1),                       // [5] gap / waveform
                        Constraint::Min(0),                          // [6] rest
                    ])
                    .split(np_inner);

                let pad = " ".repeat(art_left_pad);

                if let Some((art_w, art_h, pixels)) = crate::album_art::get_album_pixels(&playing_song.album) {
                    // ▀ half-block: top pixel = fg, bottom pixel = bg → 2× vertical resolution
                    let mut art_lines: Vec<Line> = Vec::new();
                    for term_row in 0..art_h_terminal {
                        let mut spans: Vec<Span> = Vec::new();
                        // Left-centering pad
                        if art_left_pad > 0 {
                            spans.push(Span::raw(pad.clone()));
                        }
                        let top_py = term_row * 2;
                        let bot_py = term_row * 2 + 1;
                        for col in 0..art_w_cols {
                            let src_x = (col * art_w / art_w_cols.max(1)).min(art_w.saturating_sub(1));
                            let src_y_top = (top_py * art_h / (art_h_terminal * 2).max(1)).min(art_h.saturating_sub(1));
                            let (rt, gt, bt) = pixels[src_y_top * art_w + src_x];
                            let src_y_bot = (bot_py * art_h / (art_h_terminal * 2).max(1)).min(art_h.saturating_sub(1));
                            let (rb, gb, bb) = pixels[src_y_bot * art_w + src_x];
                            spans.push(Span::styled(
                                "▀",
                                Style::default()
                                    .fg(Color::Rgb(rt, gt, bt))
                                    .bg(Color::Rgb(rb, gb, bb)),
                            ));
                        }
                        art_lines.push(Line::from(spans));
                    }
                    f.render_widget(Paragraph::new(art_lines), np_sections[0]);
                } else {
                    // Fallback placeholder box (centered)
                    let bw = art_w_cols;
                    let bh = art_h_terminal;
                    let mut img_lines: Vec<Line> = Vec::new();
                    for row in 0..bh {
                        let inner_str = if row == 0 {
                            format!("{}┌{}┐", pad, "─".repeat(bw.saturating_sub(2)))
                        } else if row == bh - 1 {
                            format!("{}└{}┘", pad, "─".repeat(bw.saturating_sub(2)))
                        } else {
                            let inner = bw.saturating_sub(2);
                            let label = "[ album art ]";
                            if row == bh / 2 && inner >= label.len() {
                                let p = (inner - label.len()) / 2;
                                format!("{}│{}{}{}│", pad, " ".repeat(p), label, " ".repeat(inner.saturating_sub(p + label.len())))
                            } else {
                                format!("{}│{}│", pad, " ".repeat(inner))
                            }
                        };
                        img_lines.push(Line::styled(inner_str, Style::default().fg(np_color)));
                    }
                    f.render_widget(Paragraph::new(img_lines), np_sections[0]);
                }



                // Song title
                f.render_widget(
                    Paragraph::new(playing_song.title.as_str())
                        .style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                    np_sections[2],
                );
                // Artist
                f.render_widget(
                    Paragraph::new(playing_song.artist.as_str())
                        .style(Style::default().fg(Color::DarkGray)),
                    np_sections[3],
                );
                // Album name
                f.render_widget(
                    Paragraph::new(playing_song.album.as_str())
                        .style(Style::default().fg(np_color)),
                    np_sections[4],
                );

                // Waveform: live amplitude tapped straight from the decoded
                // audio as rodio plays it (see AmpTap), not precomputed data.
                let wave_width = np_inner.width as usize;
                let live_samples: Vec<f32> = amp_buf
                    .lock()
                    .map(|b| b.iter().copied().collect())
                    .unwrap_or_default();
                let wave_data: Vec<u64> = if live_samples.is_empty() || wave_width == 0 {
                    // ponytail: nothing tapped yet (paused/loading) — static flat line
                    vec![0; wave_width]
                } else {
                    let chunk = (live_samples.len() / wave_width).max(1);
                    let levels: Vec<f32> = (0..wave_width)
                        .map(|i| {
                            let start = (i * chunk).min(live_samples.len());
                            let end = (start + chunk).min(live_samples.len());
                            live_samples[start..end]
                                .iter()
                                .fold(0.0f32, |m, &v| m.max(v.abs()))
                        })
                        .collect();
                    let lo = levels.iter().cloned().fold(f32::INFINITY, f32::min);
                    let hi = levels.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
                    let span = (hi - lo).max(0.01);
                    levels
                        .iter()
                        .map(|&v| (((v - lo) / span) * 8.0).round() as u64)
                        .collect()
                };
                f.render_widget(
                    ratatui::widgets::canvas::Canvas::default()
                        .marker(ratatui::symbols::Marker::Braille)
                        .x_bounds([0.0, wave_data.len().saturating_sub(1) as f64])
                        .y_bounds([0.0, 8.0])
                        .paint(|ctx| {
                            for pair in wave_data.windows(2).enumerate() {
                                let (i, w) = pair;
                                ctx.draw(&ratatui::widgets::canvas::Line {
                                    x1: i as f64,
                                    y1: w[0] as f64,
                                    x2: (i + 1) as f64,
                                    y2: w[1] as f64,
                                    color: np_color,
                                });
                            }
                        }),
                    np_sections[5],
                );

                // ---- Next in Queue block ----
                let queue_block = Block::default()
                    .title(" Next in Queue ")
                    .borders(Borders::ALL)
                    .border_type(BorderType::Plain)
                    .border_style(Style::default().fg(Color::DarkGray))
                    .padding(Padding::new(1, 1, 0, 0));

                let queue_inner = queue_block.inner(queue_rect);
                f.render_widget(queue_block, queue_rect);

                // Show the actual upcoming queue (same order "Next" will play).
                let max_queue = queue_inner.height as usize / 2; // 2 lines per track
                let playlist = &playlists[playing_playlist_idx];
                let mut queue_lines: Vec<Line> = Vec::new();

                ensure_queue(&mut upcoming_queue, playing_song_idx, playlist.songs.len());

                for &next_idx in upcoming_queue.iter().take(max_queue) {
                    let next = &playlist.songs[next_idx];
                    let max_w = queue_inner.width as usize;

                    // Title line (truncated to fit)
                    let title = if next.title.len() > max_w {
                        format!("{}…", &next.title[..max_w.saturating_sub(1)])
                    } else {
                        next.title.clone()
                    };

                    queue_lines.push(Line::from(vec![
                        Span::styled(
                            format!("{}. ", next_idx + 1),
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::styled(title, Style::default().fg(Color::White)),
                    ]));
                    queue_lines.push(Line::from(vec![
                        Span::styled(
                            format!("   {} · {}", next.artist, next.duration),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]));
                }

                if queue_lines.is_empty() {
                    queue_lines.push(Line::styled(
                        "  End of playlist",
                        Style::default().fg(Color::DarkGray),
                    ));
                }

                f.render_widget(Paragraph::new(queue_lines), queue_inner);
            }


            // ------------------ 3. Playing Bar ------------------
            let player_hovered = mouse_pos.map_or(false, |(mx, my)| contains_point(player_rect, mx, my));
            let player_border_style = if player_hovered {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let player_block = Block::default()
                .title(" Playing ")
                .borders(Borders::ALL)
                .border_type(BorderType::Plain)
                .border_style(player_border_style)
                .padding(Padding::uniform(1));

            player_inner = player_block.inner(player_rect);

            // Split player vertically into: Top section (2 lines), Spacer (1 line), and Progress bar (1 line)
            let player_rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(2),
                    Constraint::Length(1),
                    Constraint::Length(1),
                ])
                .split(player_inner);

            // Split the Top section horizontally into: Left (33%), Middle (34%), Right (33%)
            let player_top_cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(33), // Title / Artist
                    Constraint::Percentage(34), // Controls
                    Constraint::Percentage(33), // Volume
                ])
                .split(player_rows[0]);

            player_rows_save = player_rows.to_vec();
            progress_bar_rect_save = player_rows[2];
            player_top_cols_save = player_top_cols.to_vec();

            // Left Side: Title / Artist
            let player_title = Paragraph::new(vec![
                Line::from(vec![
                    Span::styled(&playing_song.title, Style::default().add_modifier(Modifier::BOLD).fg(Color::White))
                ]),
                Line::from(vec![Span::styled(&playing_song.artist, Style::default().fg(Color::DarkGray))]),
            ]).alignment(Alignment::Left);

             // Middle Side: Controls
             let play_symbol = if is_playing { "║" } else { "▶" };
             let controls_text = vec![
                 Line::from(format!("◄◄    {}    ►►", play_symbol)),
             ];
             let controls_paragraph = Paragraph::new(controls_text).alignment(Alignment::Center);
 
             // Right Side: Volume Gauge (rendered at 1 character height to match song progress bar)
             let volume_ratio = (volume as f64 / 100.0).clamp(0.0, 1.0);
             let volume_label = if volume == 0 {
                 String::from("MUTE")
             } else {
                 format!("{}%", volume)
             };
 
             let volume_gauge = Gauge::default()
                 .block(Block::default())
                 .gauge_style(Style::default().fg(Color::Green).bg(Color::Rgb(30, 30, 30)))
                 .ratio(volume_ratio)
                 .label(volume_label);
 
             let volume_layout = Layout::default()
                 .direction(Direction::Vertical)
                 .constraints([
                     Constraint::Length(1), // 1-character tall gauge row
                     Constraint::Length(1), // Spacer
                 ])
                 .split(player_top_cols[2]);
 
             let volume_gauge_layout = Layout::default()
                 .direction(Direction::Horizontal)
                 .constraints([
                     Constraint::Min(0),     // Push the gauge to the right
                     Constraint::Length(16), // Set a compact fixed width of 16 chars
                 ])
                 .split(volume_layout[0]);
 
             // Progress bar: custom render so we can show a scrubber thumb on hover
             let ratio = (play_seconds as f64 / playing_song.duration_seconds as f64).clamp(0.0, 1.0);
             let label = format!("{} / {}", format_seconds(play_seconds), playing_song.duration);
             let bar_rect = player_rows[2];
             let bar_w = bar_rect.width as usize;

             // Detect if mouse is hovering over the progress bar row specifically
             let hover_col: Option<usize> = mouse_pos.and_then(|(mx, my)| {
                 if my == bar_rect.y
                     && mx >= bar_rect.x
                     && mx < bar_rect.x + bar_rect.width
                 {
                     Some((mx - bar_rect.x) as usize)
                 } else {
                     None
                 }
             });

             let filled = ((ratio * bar_w as f64) as usize).min(bar_w);

             // Build bar char-by-char, overlaying the centered time label
             let label_chars: Vec<char> = label.chars().collect();
             let label_start = if bar_w >= label_chars.len() {
                 (bar_w - label_chars.len()) / 2
             } else {
                 bar_w // hide label if bar too narrow
             };
             let label_end = label_start + label_chars.len();

             let is_hovering = hover_col.is_some();

             let bar_spans: Vec<Span> = (0..bar_w).map(|i| {
                 // Thumb sits at the filled/unfilled boundary, shown only while hovering
                 let is_thumb = is_hovering && i == filled.min(bar_w.saturating_sub(1));
                 let in_label = i >= label_start && i < label_end;
                 let ch: String = if is_thumb {
                     "█".to_string()
                 } else if in_label {
                     label_chars[i - label_start].to_string()
                 } else if i < filled {
                     "█".to_string()
                 } else {
                     "█".to_string()
                 };
                 let style = if is_thumb {
                     Style::default().fg(Color::White)
                 } else if in_label {
                     if i < filled {
                         Style::default().fg(Color::Black).bg(Color::Green)
                     } else {
                         Style::default().fg(Color::White).bg(Color::Rgb(40, 40, 40))
                     }
                 } else if i < filled {
                     Style::default().fg(Color::Green)
                 } else {
                     Style::default().fg(Color::Rgb(40, 40, 40))
                 };
                 Span::styled(ch, style)
             }).collect();


             f.render_widget(player_title, player_top_cols[0]);
             f.render_widget(controls_paragraph, player_top_cols[1]);
             f.render_widget(volume_gauge, volume_gauge_layout[1]);
             f.render_widget(Paragraph::new(Line::from(bar_spans)), bar_rect);
             f.render_widget(player_block, player_rect);

        })?;

        // Handle Input
        if event::poll(tick_rate)? {
            match event::read()? {
                Event::Key(key) => {
                    if key.kind == event::KeyEventKind::Press {
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Esc => break,
                            KeyCode::Left => {
                                active_panel = ActivePanel::Sidebar;
                            }
                            KeyCode::Right => {
                                active_panel = ActivePanel::MainList;
                            }
                            KeyCode::Up => match active_panel {
                                ActivePanel::Sidebar => {
                                    selected_playlist_idx = selected_playlist_idx.saturating_sub(1);
                                }
                                ActivePanel::MainList => {
                                    selected_song_idx = selected_song_idx.saturating_sub(1);
                                    table_state.select(Some(selected_song_idx * 2));
                                }
                            },
                            KeyCode::Down => match active_panel {
                                ActivePanel::Sidebar => {
                                    if selected_playlist_idx + 1 < playlists.len() {
                                        selected_playlist_idx += 1;
                                    }
                                }
                                ActivePanel::MainList => {
                                    if selected_song_idx + 1 < current_playlist.songs.len() {
                                        selected_song_idx += 1;
                                        table_state.select(Some(selected_song_idx * 2));
                                    }
                                }
                            },
                            KeyCode::Enter => match active_panel {
                                ActivePanel::Sidebar => {
                                    active_playlist_idx = selected_playlist_idx;
                                    selected_song_idx = 0;
                                    table_state.select(Some(0));
                                }
                                ActivePanel::MainList => {
                                    playing_playlist_idx = active_playlist_idx;
                                    playing_song_idx = selected_song_idx;
                                    upcoming_queue.clear();
                                    play_seconds = 0;
                                    is_playing = true;
                                    play_current_song(&sink, &playlists[playing_playlist_idx], playing_song_idx, &amp_buf);
                                }
                            },
                            KeyCode::Char(' ') => {
                                is_playing = !is_playing;
                                if let Some(ref s) = sink {
                                    if is_playing {
                                        if s.empty() {
                                            play_current_song(&sink, &playlists[active_playlist_idx], selected_song_idx, &amp_buf);
                                            playing_playlist_idx = active_playlist_idx;
                                            playing_song_idx = selected_song_idx;
                                            upcoming_queue.clear();
                                        } else {
                                            s.play();
                                        }
                                    } else {
                                        s.pause();
                                    }
                                }
                            }
                            KeyCode::Char('n') | KeyCode::Char('N') => {
                                // Skip Next
                                play_seconds = 0;
                                playing_song_idx = pop_next_song(&mut upcoming_queue, playing_song_idx, playing_playlist.songs.len());
                                play_current_song(&sink, &playlists[playing_playlist_idx], playing_song_idx, &amp_buf);
                            }
                            KeyCode::Char('p') | KeyCode::Char('P') => {
                                // Previous
                                play_seconds = 0;
                                if playing_song_idx > 0 {
                                    playing_song_idx -= 1;
                                } else {
                                    playing_song_idx = playing_playlist.songs.len().saturating_sub(1);
                                }
                                play_current_song(&sink, &playlists[playing_playlist_idx], playing_song_idx, &amp_buf);
                            }
                            KeyCode::Char('-') => {
                                volume = volume.saturating_sub(5);
                                if let Some(ref s) = sink {
                                    s.set_volume(volume as f32 / 100.0);
                                }
                            }
                            KeyCode::Char('+') | KeyCode::Char('=') => {
                                volume = (volume + 5).min(100);
                                if let Some(ref s) = sink {
                                    s.set_volume(volume as f32 / 100.0);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Event::Mouse(mouse_event) => {
                    mouse_pos = Some((mouse_event.column, mouse_event.row));
                    if mouse_event.kind == MouseEventKind::Down(MouseButton::Left) {
                        let col = mouse_event.column;
                        let row = mouse_event.row;

                        // Check if click inside sidebar
                        if contains_point(sidebar_inner, col, row) {
                            active_panel = ActivePanel::Sidebar;
                            let rel_row = row.saturating_sub(sidebar_inner.y + 3) as usize; // skip headers
                            let pl_click_idx = rel_row / 2;
                            if pl_click_idx < playlists.len() {
                                selected_playlist_idx = pl_click_idx;
                                active_playlist_idx = pl_click_idx;
                                selected_song_idx = 0;
                                table_state.select(Some(0));
                            }
                        }
                        // Check if click inside player
                        else if contains_point(player_inner, col, row) {
                            let controls_y = player_rows_save[0].y;
                            if row == controls_y {
                                let middle_col = player_top_cols_save[1];
                                if col >= middle_col.x && col < middle_col.x + middle_col.width {
                                    let center_x = middle_col.x + middle_col.width / 2;
                                    if col >= center_x.saturating_sub(7) && col <= center_x.saturating_sub(3) {
                                        // Clicked Previous
                                        play_seconds = 0;
                                        if playing_song_idx > 0 {
                                            playing_song_idx -= 1;
                                        } else {
                                            playing_song_idx = playing_playlist.songs.len().saturating_sub(1);
                                        }
                                        play_current_song(&sink, &playlists[playing_playlist_idx], playing_song_idx, &amp_buf);
                                    } else if col >= center_x.saturating_sub(2) && col <= center_x + 2 {
                                        // Clicked Play/Pause
                                         is_playing = !is_playing;
                                         if let Some(ref s) = sink {
                                             if is_playing {
                                                 if s.empty() {
                                                     play_current_song(&sink, &playlists[active_playlist_idx], selected_song_idx, &amp_buf);
                                                     playing_playlist_idx = active_playlist_idx;
                                                     playing_song_idx = selected_song_idx;
                                                     upcoming_queue.clear();
                                                 } else {
                                                     s.play();
                                                 }
                                             } else {
                                                 s.pause();
                                             }
                                         }
                                    } else if col >= center_x + 3 && col <= center_x + 7 {
                                        // Clicked Next
                                        play_seconds = 0;
                                        playing_song_idx = pop_next_song(&mut upcoming_queue, playing_song_idx, playing_playlist.songs.len());
                                        play_current_song(&sink, &playlists[playing_playlist_idx], playing_song_idx, &amp_buf);
                                    }
                                }
                            }
                            // Click on progress bar → seek
                            else if row == progress_bar_rect_save.y
                                && col >= progress_bar_rect_save.x
                                && col < progress_bar_rect_save.x + progress_bar_rect_save.width
                                && progress_bar_rect_save.width > 0
                            {
                                let rel = col - progress_bar_rect_save.x;
                                let fraction = rel as f64 / progress_bar_rect_save.width as f64;
                                let seek_to = (fraction * playing_song.duration_seconds as f64) as u32;
                                play_seconds = seek_to;
                                seek_current_song(&sink, &playlists[playing_playlist_idx], playing_song_idx, seek_to, &amp_buf);
                            }
                        }

                    } else if mouse_event.kind == MouseEventKind::ScrollUp {
                        let col = mouse_event.column;
                        let row = mouse_event.row;
                        if contains_point(sidebar_rect, col, row) {
                            active_panel = ActivePanel::Sidebar;
                            selected_playlist_idx = selected_playlist_idx.saturating_sub(1);
                        }
                    } else if mouse_event.kind == MouseEventKind::ScrollDown {
                        let col = mouse_event.column;
                        let row = mouse_event.row;
                        if contains_point(sidebar_rect, col, row) {
                            active_panel = ActivePanel::Sidebar;
                            if selected_playlist_idx + 1 < playlists.len() {
                                selected_playlist_idx += 1;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    Ok(())
}

fn contains_point(rect: Rect, x: u16, y: u16) -> bool {
    x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height
}

fn format_seconds(secs: u32) -> String {
    let m = secs / 60;
    let s = secs % 60;
    format!("{}:{:02}", m, s)
}

fn format_duration(total_seconds: u32) -> String {
    let m = total_seconds / 60;
    let s = total_seconds % 60;
    if m > 60 {
        let h = m / 60;
        let rem_m = m % 60;
        format!("{} hr {} min", h, rem_m)
    } else {
        format!("{} min {} sec", m, s)
    }
}
