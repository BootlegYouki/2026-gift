mod secret_screen;
mod text_renderer;
mod embedded_data;
mod album_art;

use crossterm::{
    event::{self, Event, KeyCode, EnableMouseCapture, DisableMouseCapture, MouseEventKind, MouseButton},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Terminal,
};
use std::{
    error::Error,
    io,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

// ponytail: tiny xorshift PRNG so we don't pull in the `rand` crate for a birthday card.
struct Rng(u64);
impl Rng {
    fn new() -> Self {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64
            | 1;
        Rng(seed)
    }
    fn next_f32(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f32 / (1u64 << 53) as f32
    }
    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + self.next_f32() * (hi - lo)
    }
}

const PALETTE: [Color; 8] = [
    Color::Red,
    Color::Yellow,
    Color::Green,
    Color::Cyan,
    Color::Magenta,
    Color::LightRed,
    Color::LightYellow,
    Color::LightCyan,
];

struct Particle {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    life: f32,
    color: Color,
}

enum Firework {
    Rising { p: Particle, target_y: f32 },
    Burst { particles: Vec<Particle> },
}

fn spawn_rocket(rng: &mut Rng, width: u16, height: u16) -> Firework {
    let x = rng.range(width as f32 * 0.15, width as f32 * 0.85);
    let target_y = rng.range(height as f32 * 0.15, height as f32 * 0.5);
    Firework::Rising {
        p: Particle {
            x,
            y: height as f32 - 1.0,
            vx: rng.range(-0.15, 0.15),
            vy: -rng.range(0.9, 1.3),
            life: 1.0,
            color: Color::White,
        },
        target_y,
    }
}

fn explode(rng: &mut Rng, x: f32, y: f32) -> Firework {
    let color = PALETTE[(rng.next_f32() * PALETTE.len() as f32) as usize % PALETTE.len()];
    let count = 28 + (rng.next_f32() * 20.0) as usize;
    let mut particles = Vec::with_capacity(count);
    for _ in 0..count {
        let angle = rng.range(0.0, std::f32::consts::TAU);
        let speed = rng.range(0.3, 1.1);
        particles.push(Particle {
            x,
            y,
            vx: angle.cos() * speed,
            vy: angle.sin() * speed * 0.6,
            life: rng.range(0.85, 1.0),
            color,
        });
    }
    Firework::Burst { particles }
}

fn glyph_for_life(life: f32) -> char {
    if life > 0.66 {
        '*'
    } else if life > 0.33 {
        '+'
    } else {
        '.'
    }
}

fn tick(fireworks: &mut Vec<Firework>, rng: &mut Rng, width: u16, height: u16) {
    let mut new_bursts = Vec::new();
    fireworks.retain_mut(|fw| match fw {
        Firework::Rising { p, target_y } => {
            p.x += p.vx;
            p.y += p.vy;
            p.vy += 0.02;
            if p.y <= *target_y || p.vy >= 0.0 {
                new_bursts.push(explode(rng, p.x, p.y));
                false
            } else {
                true
            }
        }
        Firework::Burst { particles } => {
            for part in particles.iter_mut() {
                part.x += part.vx;
                part.y += part.vy;
                part.vy += 0.035;
                part.vx *= 0.98;
                part.life -= 0.025;
            }
            particles.retain(|part| part.life > 0.0 && part.y < height as f32);
            !particles.is_empty()
        }
    });
    fireworks.append(&mut new_bursts);

    if fireworks.len() < 6 && rng.next_f32() < 0.08 {
        fireworks.push(spawn_rocket(rng, width, height));
    }
}

fn render_fireworks(
    fireworks: &[Firework],
    stars: &[(u16, u16, f32)],
    time: f32,
    width: u16,
    height: u16,
) -> Vec<Line<'static>> {
    let w = width as usize;
    let h = height as usize;
    let mut grid: Vec<Option<(char, Color)>> = vec![None; w * h];

    for &(x, y, offset) in stars {
        if (x as usize) < w && (y as usize) < h {
            let twinkle = (time * 2.0 + offset).sin();
            let star = if twinkle > 0.3 {
                Some(('*', Color::White))
            } else if twinkle > -0.3 {
                Some(('.', Color::DarkGray))
            } else {
                None
            };
            if let Some(s) = star {
                grid[y as usize * w + x as usize] = Some(s);
            }
        }
    }

    for fw in fireworks {
        match fw {
            Firework::Rising { p, .. } => {
                let (x, y) = (p.x.round() as isize, p.y.round() as isize);
                if x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h {
                    grid[y as usize * w + x as usize] = Some(('|', Color::White));
                }
            }
            Firework::Burst { particles } => {
                for part in particles {
                    let (x, y) = (part.x.round() as isize, part.y.round() as isize);
                    if x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h {
                        let color = if part.life < 0.2 { Color::DarkGray } else { part.color };
                        grid[y as usize * w + x as usize] = Some((glyph_for_life(part.life), color));
                    }
                }
            }
        }
    }

    grid.chunks(w)
        .map(|row| {
            let mut spans = Vec::new();
            let mut i = 0;
            while i < row.len() {
                match row[i] {
                    Some((ch, color)) => {
                        spans.push(Span::styled(ch.to_string(), Style::default().fg(color)));
                        i += 1;
                    }
                    None => {
                        let start = i;
                        while i < row.len() && row[i].is_none() {
                            i += 1;
                        }
                        spans.push(Span::raw(" ".repeat(i - start)));
                    }
                }
            }
            Line::from(spans)
        })
        .collect()
}

// ponytail: "ANSI Shadow" figlet glyphs — the font most CLI banners (cfonts,
// figlet defaults) use. Only the letters needed for "HAPPY BIRTHDAY" + names.
fn letter_rows(c: char) -> [&'static str; 6] {
    match c.to_ascii_uppercase() {
        'A' => [
            " █████╗ ", "██╔══██╗", "███████║", "██╔══██║", "██║  ██║", "╚═╝  ╚═╝",
        ],
        'B' => [
            "██████╗ ", "██╔══██╗", "██████╔╝", "██╔══██╗", "██████╔╝", "╚═════╝ ",
        ],
        'D' => [
            "██████╗ ", "██╔══██╗", "██║  ██║", "██║  ██║", "██████╔╝", "╚═════╝ ",
        ],
        'H' => [
            "██╗  ██╗", "██║  ██║", "███████║", "██╔══██║", "██║  ██║", "╚═╝  ╚═╝",
        ],
        'I' => ["██╗", "██║", "██║", "██║", "██║", "╚═╝"],
        'M' => [
            "███╗   ███╗",
            "████╗ ████║",
            "██╔████╔██║",
            "██║╚██╔╝██║",
            "██║ ╚═╝ ██║",
            "╚═╝     ╚═╝",
        ],
        'P' => [
            "██████╗ ", "██╔══██╗", "██████╔╝", "██╔═══╝ ", "██║     ", "╚═╝     ",
        ],
        'R' => [
            "██████╗ ", "██╔══██╗", "██████╔╝", "██╔══██╗", "██║  ██║", "╚═╝  ╚═╝",
        ],
        'T' => [
            "████████╗",
            "╚══██╔══╝",
            "   ██║   ",
            "   ██║   ",
            "   ██║   ",
            "   ╚═╝   ",
        ],
        'Y' => [
            "██╗   ██╗",
            "╚██╗ ██╔╝",
            " ╚████╔╝ ",
            "  ╚██╔╝  ",
            "   ██║   ",
            "   ╚═╝   ",
        ],
        _ => ["   ", "   ", "   ", "   ", "   ", "   "],
    }
}

fn ascii_banner(text: &str) -> [String; 6] {
    let mut rows: [String; 6] = Default::default();
    for c in text.chars() {
        let glyph = letter_rows(c);
        for (row, g) in rows.iter_mut().zip(glyph.iter()) {
            row.push_str(g);
        }
    }
    rows
}

// Sunset gradient: gold -> hot pink -> violet, cycling.
const GRADIENT_STOPS: [(u8, u8, u8); 3] = [(255, 200, 0), (255, 40, 130), (150, 60, 230)];

fn gradient_color(t: f32) -> Color {
    let t = t.rem_euclid(1.0) * GRADIENT_STOPS.len() as f32;
    let i = t as usize % GRADIENT_STOPS.len();
    let j = (i + 1) % GRADIENT_STOPS.len();
    let frac = t - t.floor();
    let (r1, g1, b1) = GRADIENT_STOPS[i];
    let (r2, g2, b2) = GRADIENT_STOPS[j];
    let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * frac) as u8;
    Color::Rgb(lerp(r1, r2), lerp(g1, g2), lerp(b1, b2))
}

fn gradient_line(row: &str, phase: f32) -> Line<'static> {
    Line::from(
        row.chars()
            .enumerate()
            .map(|(x, c)| {
                let t = x as f32 * 0.03 + phase;
                Span::styled(c.to_string(), Style::default().fg(gradient_color(t)))
            })
            .collect::<Vec<_>>(),
    )
}

// Greedy word-wrap since we need pre-wrapped lines to fold into one scrollable Paragraph.
fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        if !cur.is_empty() && cur.len() + 1 + word.len() > width {
            lines.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(word);
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

#[allow(unreachable_code, unused_variables)]
fn main() -> Result<(), Box<dyn Error>> {
    let name = std::env::args().nth(1);

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut rng = Rng::new();
    let mut fireworks: Vec<Firework> = Vec::new();
    let init_size = terminal.size()?;
    let stars: Vec<(u16, u16, f32)> = (0..80)
        .map(|_| {
            (
                rng.range(0.0, init_size.width as f32) as u16,
                rng.range(0.0, init_size.height as f32) as u16,
                rng.range(0.0, std::f32::consts::TAU),
            )
        })
        .collect();
    let tick_rate = Duration::from_millis(33);
    let mut last_tick = Instant::now();
    let mut phase = 0.0f32;
    let mut scroll: u16 = 0;
    let who = name.as_deref().unwrap_or("MY BIBI");
    let message = "Happy Birthday to my precious bibi WESGOSIES! Even though it's your birthday \
                   I'm still the one who received gift, and that is YOU :0. I wish you good health \
                   and I hope you always take care of yourself. I love you always my bibi despite my \
                   shortcomings. I know you can always overcome your problems so stay strong lang \
                   always. I hope you like this birthday card :3.".to_string();
    const HEART: [&str; 33] = [
        r#"                                .,,;;;;,,,.         .,,,."#,
        r#"                  ,;;<!!!;> ,<!!!!!!!!!!!!!!!>:  :!!!!!!!!!!,"#,
        r#"                ;!!!',, ``,>' .,,.`'''````''!!!!>.`!`,,`'!!!!!"#,
        r#"                !!!  3$% ;'.c$??$$$uu$$$?$$$ec.`!!!  d$$  `!!!!"#,
        r#"                 !!>.`".;>.d"   ?$$$$$$   `?$$$% <!!> ?$$  !!!!"#,
        r#"                 `!!',!!! $$c,,c$?????$c, ,d$$$   !!!! " ,!!!'"#,
        r#"                  '';!!!! "R$$$b==""==d$$$$$$" <!!!!!; '''"#,
        r#"                   !!!!;`,cd$$F        `$$$$c. <!!!!!!!!>."#,
        r#"                  '!!;! `$$$$$$c,.  .,c$$$$$$F% !!!!!!;;!!"#,
        r#"                   `<!!>' $$$$?$$$$$$$F?$$$$$% :!!!!!!!!;;"#,
        r#"                    `_._` "$$$b,"?$P",z$P"_.,-,,._``'!!!'"#,
        r#"              _,-+'''   ``"-+,""?eee$$F,-'        ``"--,"#,
        r#"       .,,. ,'                `".`"",+'                 `+."#,
        r#"     ;!!!!!!!;                   `\/                       +"#,
        r#"    ;!!!!!!!!;                                              `."#,
        r#"    !!!!!!!!!'                                               )"#,
        r#"   `!!!!!!!!!                                                :"#,
        r#"    ``!!'                                                :<!!!!!>"#,
        r#"      ` :                                                ,!!!!!!!>"#,
        r#"        (                                                 `!!!!!!!!>"#,
        r#"         \                     ┌──────┐                  !!!!!!!!!"#,
        r#"          \                    │TAP ME│                     `!!!!!"#,
        r#"  ,;<!!!!!,`.                  └──────┘                  ,'`''  .,,,,."#,
        r#" <!!!!!!!!!!.\                                          /    :!!!!!!!!!>"#,
        r#" !!''`'!!!!!!>`\                                      /',: :!!!!!!!!!!!!!;"#,
        r#"`!<$$$$ec`<!!!!>`\                                  ,',!!'.!!!!!!!!!!!!!!!"#,
        r#"  :$$$$$$bc`!!!!!>`\                              ,',<!!!!!!!!!'.,,.`<!!!"#,
        r#"    "?$$$$$b`!!!!!!,`\                          ,',<!!!!!!!!!'4$$$$$F:!'"#,
        r#"    ': `3$$$L'!!!!!!!,`\                      /',<!!!!!!!!!!! d$P"" /'"#,
        r#"    '-J.$$$$  !!!!!!!!!,`+.                ,+';!!!!!!!!!!!!!'$$$C. /"#,
        r#"     `'?$$$%:!!!!!!!!!!!`  `+_           ,'     `'<!!!!!!!!! ?$$P ;"#,
        r#"       `-,,:!!!!!!!!!'        +.      ,-'        `''!!!!!!,,",;!"#,
        r#"         ``'''``                `-,,+'            `'!!!!!!!'"#,
    ];
    let heart_w = HEART.iter().map(|l| l.chars().count()).max().unwrap_or(0) as u16;

    loop {
        let size = terminal.size()?;
        tick(&mut fireworks, &mut rng, size.width, size.height);
        phase += 0.015;

        terminal.draw(|f| {
            let area = f.area();
            let lines = render_fireworks(&fireworks, &stars, phase, area.width, area.height);
            f.render_widget(Paragraph::new(lines), area);

            let title_rows = ascii_banner("HAPPY BIRTHDAY");
            let name_rows = ascii_banner(name.as_deref().unwrap_or("MY BIBI"));
            let banner_w = title_rows[0].len().max(name_rows[0].len());
            let text_w = 60usize.min(area.width as usize).max(1);
            let content_w = heart_w
                .max(banner_w as u16)
                .max(text_w as u16)
                .min(area.width);

            // Everything (title, name, message, heart, signature) is one tall
            // Paragraph so Up/Down scrolls it all as a single unit with no
            // artificial position clamp cutting off the bottom.
            let mut content: Vec<Line> = vec![Line::from(""); 8];
            content.extend(title_rows.iter().map(|r| gradient_line(r, phase)));
            content.push(Line::from(""));
            content.extend(name_rows.iter().map(|r| gradient_line(r, phase + 0.5)));
            content.push(Line::from(""));
            content.push(Line::from(""));
            content.extend(
                wrap_text(&message, text_w)
                    .into_iter()
                    .map(|l| Line::styled(l, Style::default().fg(Color::LightYellow))),
            );
            content.push(Line::from(""));
            content.push(Line::styled(
                "Look may Big Bear sa baba :0",
                Style::default().fg(Color::LightYellow),
            ));
            content.push(Line::from(""));
            content.push(Line::from(""));
            content.extend(HEART.iter().map(|r| {
                let padded = format!("{:<width$}", r, width = heart_w as usize);
                Line::styled(padded, Style::default().fg(Color::LightRed))
            }));
            content.push(Line::from(""));
            content.push(Line::styled(
                format!("— I wuv u always {who} —"),
                Style::default()
                    .fg(Color::LightMagenta)
                    .add_modifier(ratatui::style::Modifier::ITALIC),
            ));

            let content_area = Rect {
                x: area.x + (area.width - content_w) / 2,
                y: area.y,
                width: content_w,
                height: area.height,
            };
            f.render_widget(
                Paragraph::new(content)
                    .alignment(Alignment::Center)
                    .scroll((scroll, 0)),
                content_area,
            );
        })?;

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) => {
                    if key.kind == event::KeyEventKind::Press {
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Esc => break,
                            KeyCode::Down => scroll = scroll.saturating_add(1),
                            KeyCode::Up => scroll = scroll.saturating_sub(1),
                            KeyCode::Enter => {
                                secret_screen::run_secret_screen(&mut terminal)?;
                            }
                            _ => {}
                        }
                    }
                }
                Event::Mouse(mouse_event) => {
                    if mouse_event.kind == MouseEventKind::Down(MouseButton::Left) {
                        let col = mouse_event.column;
                        let row = mouse_event.row;

                        let title_rows = ascii_banner("HAPPY BIRTHDAY");
                        let name_rows = ascii_banner(name.as_deref().unwrap_or("MY BIBI"));
                        let banner_w = title_rows[0].len().max(name_rows[0].len());
                        let text_w = 60usize.min(size.width as usize).max(1);
                        let content_w = heart_w
                            .max(banner_w as u16)
                            .max(text_w as u16)
                            .min(size.width);

                        let msg_len = wrap_text(&message, text_w).len();
                        let button_line_index = 48 + msg_len;

                        let content_area = Rect {
                            x: size.width.saturating_sub(content_w) / 2,
                            y: 0,
                            width: content_w,
                            height: size.height,
                        };

                        let button_row = content_area.y + (button_line_index as u16) - scroll;
                        let heart_start_x = content_area.x + (content_area.width - heart_w) / 2;
                        let button_start_col = heart_start_x + 31;
                        let button_end_col = button_start_col + 8;

                        if row == button_row && col >= button_start_col && col < button_end_col {
                            secret_screen::run_secret_screen(&mut terminal)?;
                        }
                    } else if mouse_event.kind == MouseEventKind::ScrollDown {
                        scroll = scroll.saturating_add(1);
                    } else if mouse_event.kind == MouseEventKind::ScrollUp {
                        scroll = scroll.saturating_sub(1);
                    }
                }
                _ => {}
            }
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}
