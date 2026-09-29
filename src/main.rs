//! avcii-sketch: disposable teaching example. Glyph-only city ride.
//! Demo input is SIMULATED; view accepts labelled MANUAL/FAKE events only.
//! No keyboard activity capture or real agent hooks.
//!
//!   cargo run --release              live, in this terminal (q / Esc / Ctrl-C quits)
//!   cargo run --release -- --dump 8  print one plain-text frame at t=8s (no TTY needed)
//!   cargo run --release -- --trace   print how one streetlight projects vs. where a cell grid puts it

use std::io::{self, BufWriter, Write};
use std::time::{Duration, Instant};
use std::path::Path;

mod events;
mod web;

use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{self, Event, KeyCode, KeyModifiers},
    execute, queue,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};

// ───────────────────────── 1. SIMULATED input ─────────────────────────

/// SIMULATED. Stands in for "how fast I'm typing" and "is the agent working".
#[derive(Clone, Copy)]
struct SimInput {
    activity: f32, // 0.0 = not typing, 1.0 = typing hard
    agent_busy: bool,
}

/// SIMULATED 20-second loop: type, wait on the agent, type lightly, idle.
fn simulated_input(t: f32) -> SimInput {
    let p = t % 20.0;
    if p < 6.0 {
        SimInput { activity: 0.9, agent_busy: false } // typing hard
    } else if p < 14.0 {
        SimInput { activity: 0.0, agent_busy: true } // agent working: enjoy the scenery
    } else if p < 17.0 {
        SimInput { activity: 0.4, agent_busy: false } // light typing
    } else {
        SimInput { activity: 0.0, agent_busy: false } // idle
    }
}

// ───────────────────── 2. Continuous state (NOT cells) ─────────────────────
// Everything here is f32 world units / seconds. Nothing is rounded to a cell yet.

const CRUISE: f32 = 6.0; // world units per second
const TOP: f32 = 32.0;

struct Frame {
    t: f32,
    cam_z: f32,       // how far the camera has travelled down the street
    speed: f32,
    wheel_phase: f32, // continuous; a sprite frame is picked from it later
    busy: bool,
    activity: f32,
    event: Option<events::Message>,
    motion: events::Motion,
    lateral: f32, // bounded relative road position; -1 left, +1 right
}

impl Frame {
    fn new() -> Self {
        Frame { t: 0.0, cam_z: 0.0, speed: CRUISE, wheel_phase: 0.0, busy: false, activity: 0.0, event: None, motion: events::Motion::NONE, lateral: 0.0 }
    }

    fn step(&mut self, dt: f32, input: SimInput) {
        let target = if input.agent_busy { CRUISE } else { CRUISE + input.activity * (TOP - CRUISE) };
        let rate = if target > self.speed { 1.5 } else { 0.8 }; // pick up quickly, ease off slowly
        self.speed += (target - self.speed) * (1.0 - (-rate * dt).exp());
        self.cam_z += self.speed * dt;
        self.wheel_phase += self.speed * dt * 0.9;
        self.t += dt;
        self.busy = input.agent_busy;
        self.activity = input.activity;
    }

    fn step_event(&mut self, dt: f32, message: events::Message, motion: events::Motion) {
        self.event = Some(message);
        self.motion = motion;
        let target = motion.direction as f32;
        self.lateral += (target - self.lateral) * (1.0 - (-10.0 * dt).exp());
        self.step(dt, SimInput {
            activity: if motion.boost { 1.0 } else if message.state == "ACTIVE" { 0.9 } else { 0.0 },
            agent_busy: message.state == "BUSY" && !motion.boost,
        });
    }
}

// ───────────────────────── 3. Camera / projection ─────────────────────────
// World: x right, y up, z forward. Camera sits at (0, cam_h, 0) looking down +z.

const ASPECT: f32 = 0.5; // a terminal cell is ~2x taller than wide

struct Screen {
    col: f32, // FRACTIONAL cell coordinates
    row: f32,
    scale: f32, // pixels-per-world-unit-ish; bigger = closer
}

struct Projector {
    cols: f32,
    horizon: f32,
    focal: f32,
    cam_h: f32,
}

impl Projector {
    fn new(cols: u16, rows: u16) -> Self {
        let (c, r) = (cols as f32, rows as f32);
        Projector { cols: c, horizon: r * 0.36, focal: c * 0.45, cam_h: 2.4 }
    }
    fn project(&self, x: f32, y: f32, z: f32) -> Option<Screen> {
        if z < 0.8 {
            return None;
        }
        let s = self.focal / z;
        Some(Screen {
            col: self.cols * 0.5 + x * s,
            row: self.horizon - (y - self.cam_h) * s * ASPECT,
            scale: s,
        })
    }
    /// Distance ahead of the camera at which the ground (y=0) appears on `row`.
    fn ground_z(&self, row: f32) -> Option<f32> {
        let dy = row - self.horizon;
        if dy < 0.6 { None } else { Some(self.cam_h * self.focal * ASPECT / dy) }
    }
}

// ───────────────────────── 4. World: deterministic objects ─────────────────────────

const SEG: f32 = 12.0; // one street segment, world units
const ROAD_HALF: f32 = 3.5;
const WALK_X: f32 = 4.4; // streetlight x
const BLDG_X: f32 = 6.2; // building inner edge x

fn hash(i: i64, salt: u32) -> f32 {
    let mut h = (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (salt as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h ^= h >> 29;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 32;
    (h & 0xFF_FFFF) as f32 / 16_777_216.0
}

struct Building {
    side: f32,    // -1 left, +1 right
    z0: f32,      // world z of the front face
    z1: f32,      // world z of the back
    height: f32,
    width: f32,
    hue: (u8, u8, u8),
    seed: i64,
}

fn building_for(seg: i64, side: f32) -> Building {
    let salt = if side < 0.0 { 11 } else { 23 };
    let h = 10.0 + hash(seg, salt) * 26.0;
    let tint = hash(seg, salt + 1);
    let hue = if tint < 0.34 { (120, 170, 210) } else if tint < 0.67 { (200, 130, 210) } else { (110, 200, 170) };
    Building { side, z0: seg as f32 * SEG, z1: seg as f32 * SEG + SEG * 0.8, height: h, width: 9.0, hue, seed: seg * 2 + (side > 0.0) as i64 }
}

// ───────────────────────── 5. Cell grid + terminal rasterizer ─────────────────────────
// This is the ONLY place positions get rounded to cells.

#[derive(Clone, Copy, PartialEq)]
struct Cell {
    ch: char,
    fg: (u8, u8, u8),
}
const BLANK: Cell = Cell { ch: ' ', fg: (0, 0, 0) };

struct Grid {
    w: i32,
    h: i32,
    cells: Vec<Cell>,
}

impl Grid {
    fn new(w: u16, h: u16) -> Self {
        Grid { w: w as i32, h: h as i32, cells: vec![BLANK; w as usize * h as usize] }
    }
    fn set(&mut self, col: i32, row: i32, ch: char, fg: (u8, u8, u8)) {
        if col >= 0 && row >= 0 && col < self.w && row < self.h {
            self.cells[(row * self.w + col) as usize] = Cell { ch, fg };
        }
    }
    fn text(&mut self, col: i32, row: i32, s: &str, fg: (u8, u8, u8)) {
        for (i, ch) in s.chars().enumerate() {
            self.set(col + i as i32, row, ch, fg);
        }
    }
}

fn fog(rgb: (u8, u8, u8), z: f32) -> (u8, u8, u8) {
    let k = (1.0 - z / 95.0).clamp(0.12, 1.0);
    ((rgb.0 as f32 * k) as u8, (rgb.1 as f32 * k) as u8, (rgb.2 as f32 * k) as u8)
}

/// Coarser glyphs when far away: this is how a cell grid fakes "scale".
fn wall_glyph(z: f32) -> char {
    if z < 14.0 { '%' } else if z < 28.0 { '=' } else if z < 50.0 { ':' } else { '.' }
}

fn draw_ground_and_road(g: &mut Grid, f: &Frame, p: &Projector) {
    let cx = p.cols * 0.5;
    for row in (p.horizon.ceil() as i32)..g.h {
        let Some(z) = p.ground_z(row as f32 + 0.5) else { continue };
        let s = p.focal / z;
        let (l, r) = (cx - ROAD_HALF * s, cx + ROAD_HALF * s);
        let zw = f.cam_z + z; // world z of this ground row: lane paint is anchored to the world
        for col in 0..g.w {
            let c = col as f32 + 0.5;
            let (ch, rgb) = if c < l - 1.0 || c > r + 1.0 {
                ('.', (70, 70, 80)) // pavement
            } else if c < l || c > r {
                ('=', (230, 200, 90)) // kerb line
            } else if (c - cx).abs() < 0.6 * s * 0.12 + 0.5 && (zw % 8.0) < 4.0 {
                ('|', (235, 235, 235)) // dashed centre line
            } else {
                (if z < 12.0 { ',' } else { '.' }, (90, 90, 105)) // asphalt
            };
            g.set(col, row, ch, fog(rgb, z));
        }
    }
}

fn draw_sky(g: &mut Grid, p: &Projector) {
    for row in 0..(p.horizon as i32) {
        for col in 0..g.w {
            let hsh = hash((row * 997 + col) as i64, 5);
            if hsh > 0.985 {
                g.set(col, row, '.', (150, 150, 190));
            } else if hsh > 0.9985 {
                g.set(col, row, '*', (230, 230, 255));
            }
        }
    }
}

fn draw_building(g: &mut Grid, f: &Frame, p: &Projector, b: &Building) {
    let cx = p.cols * 0.5;
    let x_in = b.side * BLDG_X;
    let (za, zb) = (b.z0 - f.cam_z, b.z1 - f.cam_z);
    if zb < 0.8 {
        return;
    }
    let za = za.max(0.8);
    // Road-facing wall: one screen column at a time, solved back to a world z.
    let (Some(a), Some(bb)) = (p.project(x_in, 0.0, za), p.project(x_in, 0.0, zb)) else { return };
    let (c_lo, c_hi) = if a.col < bb.col { (a.col, bb.col) } else { (bb.col, a.col) };
    for col in (c_lo.floor() as i32)..=(c_hi.ceil() as i32) {
        let dx = (col as f32 + 0.5 - cx).abs();
        if dx < 0.5 {
            continue;
        }
        let z = (p.focal * BLDG_X / dx).clamp(za.min(zb), za.max(zb));
        let s = p.focal / z;
        let top = p.horizon - (b.height - p.cam_h) * s * ASPECT;
        let bot = p.horizon + p.cam_h * s * ASPECT;
        for row in (top.floor().max(0.0) as i32)..(bot.ceil() as i32).min(g.h) {
            let y = p.cam_h - (row as f32 + 0.5 - p.horizon) / (s * ASPECT); // world height of this cell
            let wz = z + f.cam_z; // world z, so windows are glued to the wall
            let (wi, wj) = ((wz / 2.5).floor() as i64, (y / 2.2).floor() as i64);
            let lit = hash(wi * 131 + wj * 7 + b.seed, 9) > 0.55 && ((wz / 2.5).fract() < 0.6) && ((y / 2.2).fract() < 0.6);
            let (ch, rgb) = if lit { ('#', (255, 215, 120)) } else { (wall_glyph(z), b.hue) };
            g.set(col, row, ch, fog(rgb, z));
        }
    }
    // Front face (a flat rectangle facing the camera), drawn only when there is one to see.
    if b.z0 - f.cam_z > 0.8 {
        if let (Some(t0), Some(t1)) = (p.project(x_in, b.height, za), p.project(x_in + b.side * b.width, 0.0, za)) {
            let (c0, c1) = if t0.col < t1.col { (t0.col, t1.col) } else { (t1.col, t0.col) };
            for col in (c0.floor() as i32)..(c1.ceil() as i32) {
                for row in (t0.row.floor().max(0.0) as i32)..(t1.row.ceil() as i32).min(g.h) {
                    g.set(col, row, wall_glyph(za + 6.0), fog((b.hue.0 / 2, b.hue.1 / 2, b.hue.2 / 2), za));
                }
            }
        }
    }
}

fn draw_lamp(g: &mut Grid, p: &Projector, side: f32, z: f32) {
    let x = side * WALK_X;
    let (Some(base), Some(top)) = (p.project(x, 0.0, z), p.project(x, 5.2, z)) else { return };
    let col = top.col.round() as i32;
    let (r0, r1) = (top.row.round() as i32, base.row.round() as i32);
    if top.scale > 3.0 {
        for row in (r0 + 1)..=r1 {
            g.set(col, row, '|', fog((120, 120, 130), z));
        }
    }
    // Head glyph changes with distance; the glow pokes toward the road.
    let head = if top.scale > 10.0 { '@' } else if top.scale > 4.0 { 'O' } else { '*' };
    g.set(col, r0, head, fog((255, 235, 150), z * 0.4));
    if top.scale > 6.0 {
        g.set(col - side as i32, r0, '-', fog((255, 220, 120), z * 0.5));
    }
}

fn draw_scene(g: &mut Grid, f: &Frame, p: &Projector) {
    draw_sky(g, p);
    draw_ground_and_road(g, f, p);
    let first = (f.cam_z / SEG).floor() as i64 - 1;
    let last = ((f.cam_z + 95.0) / SEG).ceil() as i64;
    for seg in (first..=last).rev() {
        // far → near (painter's algorithm)
        for side in [-1.0f32, 1.0] {
            draw_building(g, f, p, &building_for(seg, side));
        }
        let lz = seg as f32 * SEG + SEG * 0.5 - f.cam_z;
        if lz > 0.8 {
            draw_lamp(g, p, -1.0, lz);
            draw_lamp(g, p, 1.0, lz);
        }
    }
}

// ───────────────────────── 6. Rider: hand-written glyph frames ─────────────────────────
// Original cat rider, seen from behind. Space outside the silhouette is transparent.

const HEAD_UP: [&str; 4] = [
    r"    /\___/\    ",
    r"   /       \   ",
    r"   |  |||  |   ",
    r"    \_____/    ",
];
const HEAD_LOOK_L: [&str; 4] = [
    r"    /\___/\    ",
    r"   /       \   ",
    r"  < o  ||  |   ",
    r"    \_____/    ",
];
const HEAD_LOOK_R: [&str; 4] = [
    r"    /\___/\    ",
    r"   /       \   ",
    r"   |  ||  o >  ",
    r"    \_____/    ",
];
const BODY_UP: [&str; 4] = [
    " ___/  |  \\___ ",
    "/   \\_/#\\_/   \\",
    "\\____/ # \\____/",
    "      |#|      ",
];
// Fast: head ducks between the shoulders, arms stretched forward.
const HEAD_TUCK: [&str; 2] = [
    r"    /\___/\    ",
    r"  __\_|||_/__  ",
];
const BODY_TUCK: [&str; 3] = [
    " /__\\_____/__\\ ",
    "  \\___\\#/___/  ",
    "      |#|      ",
];
const BIKE_A: [&str; 4] = [
    "    [==@==]    ",
    "     |:::|     ",
    "     |:::|     ",
    "     '---'     ",
];
const BIKE_B: [&str; 4] = [
    "    [==@==]    ",
    "     |.:.|     ",
    "     |:.:|     ",
    "     '---'     ",
];

fn rider_rows(f: &Frame) -> Vec<&'static str> {
    let fast = f.speed > 17.0;
    let bike = if (f.wheel_phase as i64) % 2 == 0 { BIKE_A } else { BIKE_B };
    let mut rows: Vec<&'static str> = Vec::new();
    if fast {
        rows.extend(HEAD_TUCK);
        rows.extend(BODY_TUCK);
    } else {
        // Relaxed: while the agent works the rider glances around.
        let head = if f.busy {
            match ((f.t / 1.5) as i64) % 4 { 0 => HEAD_LOOK_L, 2 => HEAD_LOOK_R, _ => HEAD_UP }
        } else {
            HEAD_UP
        };
        rows.extend(head);
        rows.extend(BODY_UP);
    }
    rows.extend(bike);
    rows
}

fn draw_rider(g: &mut Grid, f: &Frame) {
    let rows = rider_rows(f);
    // Whole-cell sway. In a cell grid this can only be -1/0/+1, so it visibly "steps".
    let sway = ((f.t * 0.8).sin() * (f.speed / TOP) * 1.6).round() as i32;
    let top = g.h - rows.len() as i32 - 1;
    let dodge = (f.lateral * (g.w as f32 * 0.12).min(12.0)).round() as i32;
    let left = g.w / 2 - 7 + sway + dodge;
    // Tail attaches beside the jacket; its tip swings with simulated time.
    let tail_left = (f.t * 2.0).sin() < 0.0;
    let tail = if tail_left { [r"~\", r"  \", r"   )"] } else { [r"/~", r"/", r"("] };
    let tail_col = if tail_left { left - 3 } else { left + 14 };
    for (i, line) in tail.iter().enumerate() {
        for (j, ch) in line.chars().enumerate() {
            if ch != ' ' {
                g.set(tail_col + j as i32, g.h - 8 + i as i32, ch, (255, 190, 100));
            }
        }
    }
    let head_rows = if f.speed > 17.0 { HEAD_TUCK.len() } else { HEAD_UP.len() };
    for (i, line) in rows.iter().enumerate() {
        let chars: Vec<char> = line.chars().collect();
        let first = chars.iter().position(|c| *c != ' ');
        let last = chars.iter().rposition(|c| *c != ' ');
        let (Some(first), Some(last)) = (first, last) else { continue };
        for j in first..=last {
            let ch = chars[j];
            // Spaces inside the silhouette are opaque, so the road doesn't show through the rider.
            let rgb = if i < head_rows { (255, 205, 130) } else { match ch {
                '@' => (255, 60, 60),  // tail light
                '#' => (90, 190, 255), // jacket
                _ => (235, 235, 240),
            }};
            g.set(left + j as i32, top + i as i32, ch, rgb);
        }
    }
}

fn draw_sky_sign(g: &mut Grid) {
    // Hand-drawn ASCII interpretation of the NVIDIA eye, not a bitmap logo.
    // Keep the floating sign above the road and hide it in cramped terminals.
    if g.w < 60 || g.h < 24 {
        return;
    }
    let sign = [
        r"    ___________    ",
        r"  /  __/___    \   ",
        r" <  /  (@) \___/   ",
        r"  \_\_____/____    ",
        r"      NVIDIA       ",
    ];
    for (i, line) in sign.iter().enumerate() {
        g.text(g.w / 2 - 9, 3 + i as i32, line, (118, 185, 0));
    }
}

fn draw_chrome(g: &mut Grid, f: &Frame) {
    let edge = (100, 180, 190);
    for col in 0..g.w {
        g.set(col, 0, '-', edge);
        g.set(col, g.h - 1, '-', edge);
        // Clear the HUD strip so buildings cannot bleed through its text.
        g.set(col, 1, ' ', edge);
    }
    for row in 0..g.h {
        g.set(0, row, '|', edge);
        g.set(g.w - 1, row, '|', edge);
    }
    for col in [0, g.w - 1] {
        for row in [0, g.h - 1] {
            g.set(col, row, '+', edge);
        }
    }
    let state = if let Some(message) = f.event { message.state } else if f.busy { "CRUISE" } else if f.activity > 0.5 {
        "ZOOM"
    } else if f.activity > 0.0 { "STROLL" } else { "IDLE" };
    let hud = if let Some(message) = f.event {
        format!("EVENT source={} state={} speed={:>4.1} steer={:+.2} turn={} boost={}",
            message.source, message.state, f.speed, f.lateral,
            match f.motion.direction { -1 => "LEFT", 1 => "RIGHT", _ => "CENTER" }, f.motion.boost)
    } else {
        format!("SIMULATED input  activity={:.2}  agent_busy={}  speed={:>4.1}",
            f.activity, f.busy, f.speed)
    };
    let footer = if f.event.is_some() {
        format!(" CAT: {} | steer:{} boost:{} | q/Esc/Ctrl-C: quit ", state, f.motion.steer_source, f.motion.boost_source)
    } else { format!(" CAT: {} | q / Esc / Ctrl-C: quit ", state) };
    for (row, label) in [(0, " REJAR PET / CITY RIDE "), (1, hud.as_str()), (g.h - 1, footer.as_str())] {
        for (i, ch) in label.chars().take((g.w - 4).max(0) as usize).enumerate() {
            g.set(2 + i as i32, row, ch, (220, 240, 240));
        }
    }
}

fn render(f: &Frame, cols: u16, rows: u16) -> Grid {
    let mut g = Grid::new(cols, rows);
    let p = Projector::new(cols, rows);
    draw_scene(&mut g, f, &p);
    draw_sky_sign(&mut g);
    draw_rider(&mut g, f);
    if let Some(message) = f.event {
        let feedback = match message.state {
            "DONE" => Some(("* PURR! DONE *", (140, 255, 150))),
            "ERROR" => Some(("! TASK ERROR !", (255, 120, 100))),
            "CANCELLED" => Some(("TASK CANCELLED", (240, 205, 120))),
            "DISCONNECTED" => Some(("NO EVENT LINK", (180, 180, 180))),
            _ => None,
        };
        if let Some((label, color)) = feedback {
            let row = g.h - rider_rows(f).len() as i32 - 3;
            if row > 7 { g.text(g.w / 2 - 6, row, label, color); }
        }
    }
    draw_chrome(&mut g, f);
    g
}

// ───────────────────────── 7. Terminal backend (diffed) ─────────────────────────

fn present(out: &mut impl Write, now: &Grid, prev: &mut Option<Grid>) -> io::Result<()> {
    let mut last_color: Option<(u8, u8, u8)> = None;
    let mut cursor: Option<(i32, i32)> = None;
    for row in 0..now.h {
        for col in 0..now.w {
            let i = (row * now.w + col) as usize;
            let c = now.cells[i];
            let same = prev.as_ref().map_or(false, |p| p.w == now.w && p.h == now.h && p.cells[i] == c);
            if same {
                continue;
            }
            if cursor != Some((col, row)) {
                queue!(out, MoveTo(col as u16, row as u16))?;
            }
            if last_color != Some(c.fg) {
                queue!(out, SetForegroundColor(Color::Rgb { r: c.fg.0, g: c.fg.1, b: c.fg.2 }))?;
                last_color = Some(c.fg);
            }
            queue!(out, Print(c.ch))?;
            cursor = Some((col + 1, row));
        }
    }
    queue!(out, ResetColor)?;
    out.flush()?;
    *prev = Some(Grid { w: now.w, h: now.h, cells: now.cells.clone() });
    Ok(())
}

struct TermGuard;
impl Drop for TermGuard {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), ResetColor, Show, LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}

fn run_terminal(mut feed: Option<events::Feed>) -> io::Result<()> {
    terminal::enable_raw_mode()?;
    // Establish cleanup before any further fallible terminal setup.
    let _guard = TermGuard;
    execute!(io::stdout(), EnterAlternateScreen, Hide)?;
    let mut out = BufWriter::with_capacity(64 * 1024, io::stdout());
    let mut frame = Frame::new();
    let mut prev: Option<Grid> = None;
    let mut last = Instant::now();
    let frame_interval = Duration::from_millis(33);
    let mut next_frame = last + frame_interval;
    loop {
        // Input may wake poll early; only the deadline permits another frame.
        if event::poll(next_frame.saturating_duration_since(Instant::now()))? {
            if let Event::Key(k) = event::read()? {
                let quit = matches!(k.code, KeyCode::Char('q') | KeyCode::Esc)
                    || (k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL));
                if quit {
                    return Ok(());
                }
            }
        }
        let now = Instant::now();
        if now < next_frame {
            continue;
        }
        next_frame = now + frame_interval;
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        if let Some(feed) = feed.as_mut() {
            feed.poll(now)?;
            frame.step_event(dt, feed.state.snapshot(now), feed.state.motion(now));
        } else {
            frame.step(dt, simulated_input(frame.t));
        }
        let (cols, rows) = terminal::size()?;
        let grid = render(&frame, cols, rows);
        present(&mut out, &grid, &mut prev)?;
    }
}

// ───────────────────────── 8. Headless helpers ─────────────────────────

fn observe_events(mut feed: events::Feed, seconds: f32) -> io::Result<()> {
    let start = Instant::now();
    let mut last = start;
    let mut previous = None;
    let mut frame = Frame::new();
    while start.elapsed().as_secs_f32() < seconds {
        let now = Instant::now();
        feed.poll(now)?;
        let message = feed.state.snapshot(now);
        let motion = feed.state.motion(now);
        frame.step_event(now.duration_since(last).as_secs_f32().min(0.1), message, motion);
        last = now;
        if previous != Some((message, motion)) {
            // Exercise the same renderer as view; emit plain text only on state changes.
            let grid = render(&frame, 100, 30);
            let hud: String = grid.cells[100..200].iter().map(|c| c.ch).collect();
            println!("{hud}");
            io::stdout().flush()?;
            previous = Some((message, motion));
        }
        std::thread::sleep(Duration::from_millis(33));
    }
    Ok(())
}

fn frame_at(secs: f32) -> Frame {
    let mut f = Frame::new();
    while f.t < secs {
        f.step(1.0 / 30.0, simulated_input(f.t));
    }
    f
}

fn dump(secs: f32) {
    let f = frame_at(secs);
    let g = render(&f, 100, 30);
    for row in 0..g.h {
        let line: String = (0..g.w).map(|c| g.cells[(row * g.w + c) as usize].ch).collect();
        println!("{}", line.trim_end());
    }
}

/// Shows what exists BEFORE quantization: the lamp's exact projected position vs. the cell it lands in.
fn trace() {
    let p = Projector::new(100, 30);
    let mut f = Frame::new();
    println!("SIMULATED speed ramp; one left streetlight at world z=60 (seg-anchored), 100x30 cells");
    println!("{:>5} {:>6} {:>7} {:>8} {:>8} {:>7} {:>9}", "t", "speed", "dist_z", "col", "row", "scale", "cell(c,r)");
    let mut last_cell = (0, 0);
    let mut n = 0;
    while f.t < 3.0 {
        f.step(1.0 / 30.0, simulated_input(f.t));
        n += 1;
        if n % 3 != 0 {
            continue; // print every 3rd frame (0.1 s)
        }
        if let Some(s) = p.project(-WALK_X, 5.2, 60.0 - f.cam_z) {
            let cell = (s.col.round() as i32, s.row.round() as i32);
            let moved = if cell != last_cell { " <- cell changed" } else { "" };
            last_cell = cell;
            println!("{:5.2} {:6.1} {:7.2} {:8.2} {:8.2} {:7.2} {:>4},{:<4}{}", f.t, f.speed, 60.0 - f.cam_z, s.col, s.row, s.scale, cell.0, cell.1, moved);
        }
    }
}

fn cli_args(args: impl Iterator<Item = std::ffi::OsString>) -> io::Result<Vec<String>> {
    args.map(|arg| arg.into_string().map_err(|_| io::Error::new(
        io::ErrorKind::InvalidInput,
        "argument is not valid UTF-8; retype the input or check the terminal encoding",
    ))).collect()
}

fn main() -> io::Result<()> {
    let args = cli_args(std::env::args_os().skip(1))?;
    if args.len() == 1 && matches!(args[0].as_str(), "--help" | "-h") {
        println!("avcii-sketch — SIMULATED city ride\n\
Usage: avcii-sketch [demo | view DIR [--headless SECONDS] | emit DIR SOURCE STATE]\n\
                   [say DIR TEXT]\n\
                   [web [PORT] [--seconds SECONDS]]\n\
                   [--dump SECONDS | --trace | --help | --version]\n\
  demo          Run in an interactive terminal (also the default).\n\
                q / Esc / Ctrl-C quits. All ride activity is SIMULATED.\n\
  --dump SEC    Print one plain-text frame; finite SEC in [0, 600].\n\
  view DIR      Listen in a private 0700 directory; q / Esc / Ctrl-C quits.\n\
                --headless SEC: text state changes only, finite SEC in (0, 60].\n\
  emit DIR SOURCE STATE\n\
                SOURCE: manual|fake|preview|chat.\n\
                STATE: idle|busy|done|error|cancelled|active.\n\
                Actions: boost (2s), left/right (0.9s), center.\n\
                Repeat within 5s to stay connected; done feedback lasts 2s.\n\
  say DIR TEXT  Manual text: 提速 / 向左闪避 / 向右闪避 / 回中\n\
                Also accepts boost/left/right/center; no LLM or text capture.\n\
  web [PORT]    Local chat panel at 127.0.0.1 (default port 8765); no model.\n\
                Optional --seconds SEC in (0,60] for bounded checks.\n\
  --trace       Print simulated streetlight projection data.\n\
  --help, -h    Show this help without opening the terminal UI.\n\
  --version     Print version.");
        return Ok(());
    }
    if args.len() == 1 && args[0] == "--version" {
        println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("web") {
        if !matches!(args.len(), 1 | 2 | 4) || (args.len() == 4 && args[2] != "--seconds") {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "use web [PORT] [--seconds SECONDS]"));
        }
        let port = args.get(1).map(|s| s.parse::<u16>()).transpose()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid port"))?.unwrap_or(8765);
        let seconds = if args.len() == 4 {
            Some(args[3].parse::<f32>().ok().filter(|s| s.is_finite() && *s > 0.0 && *s <= 60.0)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seconds must be in (0,60]"))?)
        } else { None };
        return web::run(port, seconds);
    }
    if args.first().map(String::as_str) == Some("view") {
        if args.len() == 2 {
            return run_terminal(Some(events::Feed::bind(Path::new(&args[1]))?));
        }
        if args.len() == 4 && args[2] == "--headless" {
            let seconds = args[3].parse::<f32>().ok()
                .filter(|s| s.is_finite() && *s > 0.0 && *s <= 60.0)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput,
                    "--headless requires finite seconds in (0, 60]"))?;
            return observe_events(events::Feed::bind(Path::new(&args[1]))?, seconds);
        }
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "use view DIR [--headless SECONDS]"));
    }
    if args.first().map(String::as_str) == Some("emit") && args.len() == 4 {
        return events::emit(Path::new(&args[1]), &args[2], &args[3]);
    }
    if args.first().map(String::as_str) == Some("say") && args.len() == 3 {
        return events::emit(Path::new(&args[1]), "manual", events::text_action(&args[2])?);
    }
    if args.first().map(String::as_str) == Some("--dump") {
        // Bound simulation work and avoid non-finite / stalled f32 time loops.
        let secs = args.get(1)
            .and_then(|s| s.parse::<f32>().ok())
            .filter(|s| args.len() == 2 && s.is_finite() && (0.0..=600.0).contains(s))
            .ok_or_else(|| io::Error::new(
                io::ErrorKind::InvalidInput,
                "--dump requires a finite number of seconds in [0, 600]",
            ))?;
        dump(secs);
        return Ok(());
    }
    if args.len() == 1 && args[0] == "--trace" {
        trace();
        return Ok(());
    }
    if args.is_empty() || (args.len() == 1 && args[0] == "demo") {
        return run_terminal(None);
    }
    Err(io::Error::new(io::ErrorKind::InvalidInput, "unknown arguments; use --help"))
}

#[cfg(test)]
mod motion_tests {
    use super::*;

    #[test]
    fn malformed_utf8_is_an_error_instead_of_a_panic() {
        use std::os::unix::ffi::OsStringExt;
        let bad = std::ffi::OsString::from_vec(vec![0xe5, 0x81]);
        assert_eq!(cli_args([bad].into_iter()).unwrap_err().kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn boost_and_dodge_move_then_recover_without_changing_task_status() {
        let mut frame = Frame::new();
        let busy = events::Message::parse("fake busy").unwrap();
        let motion = events::Motion { direction: -1, boost: true, steer_source: "MANUAL", boost_source: "MANUAL" };
        for _ in 0..30 { frame.step_event(1.0 / 30.0, busy, motion); }
        assert!(frame.speed > 17.0 && frame.speed <= TOP);
        assert!(frame.lateral < -0.9 && frame.lateral >= -1.0);
        assert_eq!(frame.event.unwrap().state, "BUSY");
        let boosted_speed = frame.speed;
        for _ in 0..60 { frame.step_event(1.0 / 30.0, busy, events::Motion::NONE); }
        assert!(frame.lateral.abs() < 0.001);
        assert!(frame.speed < boosted_speed);
        for _ in 0..30 {
            frame.step_event(1.0 / 30.0, busy, events::Motion { direction: 1, ..events::Motion::NONE });
        }
        assert!(frame.lateral > 0.9 && frame.lateral <= 1.0);
    }
}
