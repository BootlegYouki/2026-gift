#![allow(dead_code)]
use std::error::Error;
use std::io;
use std::time::{Duration, Instant};
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

fn play_current_song(sink: &Option<rodio::Sink>, playlist: &Playlist, song_idx: usize) {
    if let Some(ref s) = sink {
        s.stop();
        if song_idx < playlist.songs.len() {
            let song = &playlist.songs[song_idx];
            if let Some(bytes) = crate::embedded_data::get_embedded_audio(&playlist.name, (song_idx + 1) as u32, &song.title) {
                let cursor = std::io::Cursor::new(bytes);
                if let Ok(source) = rodio::Decoder::new(cursor) {
                    s.append(source);
                    s.play();
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
                playing_song_idx = (playing_song_idx + 1) % playing_playlist.songs.len();
                play_current_song(&sink, &playlists[playing_playlist_idx], playing_song_idx);
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

            // Body layout: Horizontal split between Sidebar (Library) and Main content
            let body_layout = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(18),
                    Constraint::Percentage(82),
                ])
                .split(main_layout[0]);

            sidebar_rect = body_layout[0];
            main_rect = body_layout[1];

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
                Line::from(vec![Span::styled("Your Library", Style::default().add_modifier(Modifier::BOLD).fg(Color::White))]),
                Line::from(vec![Span::styled("Playlists", Style::default().fg(Color::DarkGray))]),
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
                        main_content_lines.push(Line::styled(sliced, Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)));
                    }
                } else {
                    for line in shadow_lines {
                        main_content_lines.push(Line::styled(line, Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)));
                    }
                }
            } else {
                main_content_lines.push(Line::from(vec![Span::styled(&current_playlist.name, Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))]));
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
 
             // Progress Bar (bottom row of player, spanning the full width of player_inner)
             let ratio = (play_seconds as f64 / playing_song.duration_seconds as f64).clamp(0.0, 1.0);
             let label = format!("{} / {}", format_seconds(play_seconds), playing_song.duration);
             let progress_gauge = Gauge::default()
                 .block(Block::default())
                 .gauge_style(Style::default().fg(Color::Green).bg(Color::Rgb(30, 30, 30)))
                 .ratio(ratio)
                 .label(label);
 
             f.render_widget(player_title, player_top_cols[0]);
             f.render_widget(controls_paragraph, player_top_cols[1]);
             f.render_widget(volume_gauge, volume_gauge_layout[1]);
             f.render_widget(progress_gauge, player_rows[2]);
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
                                    play_seconds = 0;
                                    is_playing = true;
                                    play_current_song(&sink, &playlists[playing_playlist_idx], playing_song_idx);
                                }
                            },
                            KeyCode::Char(' ') => {
                                is_playing = !is_playing;
                                if let Some(ref s) = sink {
                                    if is_playing {
                                        s.play();
                                    } else {
                                        s.pause();
                                    }
                                }
                            }
                            KeyCode::Char('n') | KeyCode::Char('N') => {
                                // Skip Next
                                play_seconds = 0;
                                playing_song_idx = (playing_song_idx + 1) % playing_playlist.songs.len();
                                play_current_song(&sink, &playlists[playing_playlist_idx], playing_song_idx);
                            }
                            KeyCode::Char('p') | KeyCode::Char('P') => {
                                // Previous
                                play_seconds = 0;
                                if playing_song_idx > 0 {
                                    playing_song_idx -= 1;
                                } else {
                                    playing_song_idx = playing_playlist.songs.len().saturating_sub(1);
                                }
                                play_current_song(&sink, &playlists[playing_playlist_idx], playing_song_idx);
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
                                        play_current_song(&sink, &playlists[playing_playlist_idx], playing_song_idx);
                                    } else if col >= center_x.saturating_sub(2) && col <= center_x + 2 {
                                        // Clicked Play/Pause
                                        is_playing = !is_playing;
                                        if let Some(ref s) = sink {
                                            if is_playing {
                                                s.play();
                                            } else {
                                                s.pause();
                                            }
                                        }
                                    } else if col >= center_x + 3 && col <= center_x + 7 {
                                        // Clicked Next
                                        play_seconds = 0;
                                        playing_song_idx = (playing_song_idx + 1) % playing_playlist.songs.len();
                                        play_current_song(&sink, &playlists[playing_playlist_idx], playing_song_idx);
                                    }
                                }
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
