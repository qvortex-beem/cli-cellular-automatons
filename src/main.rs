//! Conway's Game of Life с бесконечным миром, разбитым на чанки (32×32).
//! Оптимизирован: битовые чанки, параллельный тик.
//! Поддерживает масштабирование: + / - (или = / -) переключают режим отображения.
//! zoom = 1 – обычное отображение (#), zoom >= 2 – символы Брайля (блок 2x4 клетки).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    crossterm::event::{self, Event, KeyCode, MouseButton, MouseEventKind},
};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::io::{self, Write};
use std::time::{Duration, Instant};

// ---------- Константы ----------
const CHUNK_SIZE: usize = 32;
const CHUNK_BITS: usize = CHUNK_SIZE * CHUNK_SIZE; // 1024
const BITS_PER_U64: usize = 64;
const U64_COUNT: usize = CHUNK_BITS / BITS_PER_U64; // 16

const SURVIVAL: usize = 2;
const BIRTH: usize = 3;

// ---------- Типы координат ----------
#[derive(Eq, Hash, PartialEq, Clone, Copy, Debug)]
pub struct Cord {
    pub x: i64,
    pub y: i64,
}

#[derive(Eq, Hash, PartialEq, Clone, Copy, Debug)]
struct ChunkCord {
    chunk_x: i64,
    chunk_y: i64,
}

// ---------- Чанк (битовая маска) ----------
#[derive(Clone)]
struct Chunk {
    bits: [u64; U64_COUNT],
}

impl Chunk {
    fn new() -> Self {
        Self {
            bits: [0; U64_COUNT],
        }
    }

    #[inline(always)]
    fn get_local(&self, x: usize, y: usize) -> bool {
        debug_assert!(x < CHUNK_SIZE && y < CHUNK_SIZE);
        let idx = y * CHUNK_SIZE + x;
        (self.bits[idx / BITS_PER_U64] >> (idx % BITS_PER_U64)) & 1 == 1
    }

    #[inline(always)]
    fn set_local(&mut self, x: usize, y: usize, alive: bool) {
        debug_assert!(x < CHUNK_SIZE && y < CHUNK_SIZE);
        let idx = y * CHUNK_SIZE + x;
        let mask = 1u64 << (idx % BITS_PER_U64);
        if alive {
            self.bits[idx / BITS_PER_U64] |= mask;
        } else {
            self.bits[idx / BITS_PER_U64] &= !mask;
        }
    }

    fn count_alive(&self) -> u32 {
        self.bits.iter().map(|&b| b.count_ones()).sum()
    }
}

// ---------- Камера ----------
#[derive(Debug, Clone, Copy)]
pub struct Camera {
    pub pos_x: i64,
    pub pos_y: i64,
    pub zoom: i64,
    last_move: Option<Instant>,
}

impl Camera {
    pub fn new() -> Self {
        Self {
            pos_x: 0,
            pos_y: 0,
            zoom: 1,
            last_move: None,
        }
    }

    pub fn world_to_buffer(&self, world_x: i64, world_y: i64, rect: Rect) -> Option<(u16, u16)> {
        let screen_x = (world_x - self.pos_x) as i16;
        let screen_y = (world_y - self.pos_y) as i16;
        if screen_x >= 0
            && (screen_x as u16) < rect.width
            && screen_y >= 0
            && (screen_y as u16) < rect.height
        {
            Some((rect.x + screen_x as u16, rect.y + screen_y as u16))
        } else {
            None
        }
    }

    pub fn try_move(&mut self, dx: i64, dy: i64) -> bool {
        let now = Instant::now();
        if let Some(last) = self.last_move {
            if now.duration_since(last) < Duration::from_millis(50) {
                return false;
            }
        }
        self.pos_x = self.pos_x.saturating_add(dx);
        self.pos_y = self.pos_y.saturating_add(dy);
        self.last_move = Some(now);
        true
    }

    pub fn buffer_to_world(&self, col: u16, row: u16, rect: Rect) -> (i64, i64) {
        let rel_x = col as i64 - rect.x as i64;
        let rel_y = row as i64 - rect.y as i64;
        (self.pos_x + rel_x, self.pos_y + rel_y)
    }

    pub fn zoom_in(&mut self) {
        if self.zoom < 4 {
            self.zoom += 1;
        }
    }

    pub fn zoom_out(&mut self) {
        if self.zoom > 1 {
            self.zoom -= 1;
        }
    }
}

// ---------- Мир ----------
pub struct World {
    chunks: HashMap<ChunkCord, Chunk>,
}

impl World {
    pub fn new() -> Self {
        Self {
            chunks: HashMap::new(),
        }
    }

    fn get_chunk_cord(x: i64, y: i64) -> ChunkCord {
        ChunkCord {
            chunk_x: x.div_euclid(CHUNK_SIZE as i64),
            chunk_y: y.div_euclid(CHUNK_SIZE as i64),
        }
    }

    fn local_coords(x: i64, y: i64) -> (usize, usize) {
        let cx = x.rem_euclid(CHUNK_SIZE as i64) as usize;
        let cy = y.rem_euclid(CHUNK_SIZE as i64) as usize;
        (cx, cy)
    }

    pub fn is_cell_alive(&self, x: i64, y: i64) -> bool {
        let chunk_cord = Self::get_chunk_cord(x, y);
        if let Some(chunk) = self.chunks.get(&chunk_cord) {
            let (lx, ly) = Self::local_coords(x, y);
            chunk.get_local(lx, ly)
        } else {
            false
        }
    }

    pub fn animate_cell(&mut self, x: i64, y: i64) {
        let chunk_cord = Self::get_chunk_cord(x, y);
        let chunk = self.chunks.entry(chunk_cord).or_insert_with(Chunk::new);
        let (lx, ly) = Self::local_coords(x, y);
        chunk.set_local(lx, ly, true);
    }

    pub fn kill_cell(&mut self, x: i64, y: i64) {
        let chunk_cord = Self::get_chunk_cord(x, y);
        if let Some(chunk) = self.chunks.get_mut(&chunk_cord) {
            let (lx, ly) = Self::local_coords(x, y);
            chunk.set_local(lx, ly, false);
            if chunk.count_alive() == 0 {
                self.chunks.remove(&chunk_cord);
            }
        }
    }

    fn animate_block(&mut self, x: i64, y: i64) {
        for dx in 0..2 {
            for dy in 0..4 {
                self.animate_cell(x + dx, y + dy);
            }
        }
    }

    fn kill_block(&mut self, x: i64, y: i64) {
        for dx in 0..2 {
            for dy in 0..4 {
                self.kill_cell(x + dx, y + dy);
            }
        }
    }

    pub fn tick(&mut self) {
        let mut affected = HashSet::new();
        for &chunk_cord in self.chunks.keys() {
            affected.insert(chunk_cord);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    affected.insert(ChunkCord {
                        chunk_x: chunk_cord.chunk_x + dx,
                        chunk_y: chunk_cord.chunk_y + dy,
                    });
                }
            }
        }

        let chunks_ref = &self.chunks;
        let next_cells: Vec<(ChunkCord, Vec<Cord>)> = affected
            .par_iter()
            .map(|&chunk_cord| {
                let mut local_alive = HashSet::new();
                for dx in -1..=1 {
                    for dy in -1..=1 {
                        let neighbor = ChunkCord {
                            chunk_x: chunk_cord.chunk_x + dx,
                            chunk_y: chunk_cord.chunk_y + dy,
                        };
                        if let Some(chunk) = chunks_ref.get(&neighbor) {
                            let offset_x = neighbor.chunk_x * CHUNK_SIZE as i64;
                            let offset_y = neighbor.chunk_y * CHUNK_SIZE as i64;
                            for idx in 0..CHUNK_BITS {
                                if (chunk.bits[idx / BITS_PER_U64] >> (idx % BITS_PER_U64)) & 1 == 1
                                {
                                    let x = (idx % CHUNK_SIZE) as i64;
                                    let y = (idx / CHUNK_SIZE) as i64;
                                    local_alive.insert(Cord {
                                        x: offset_x + x,
                                        y: offset_y + y,
                                    });
                                }
                            }
                        }
                    }
                }

                let x_start = chunk_cord.chunk_x * CHUNK_SIZE as i64;
                let y_start = chunk_cord.chunk_y * CHUNK_SIZE as i64;
                let x_end = x_start + CHUNK_SIZE as i64;
                let y_end = y_start + CHUNK_SIZE as i64;

                let mut new_cells = Vec::with_capacity(256);
                for x in x_start..x_end {
                    for y in y_start..y_end {
                        let cord = Cord { x, y };
                        let is_alive = local_alive.contains(&cord);
                        let mut neighbors = 0;
                        for dx in -1..=1 {
                            for dy in -1..=1 {
                                if dx == 0 && dy == 0 {
                                    continue;
                                }
                                if local_alive.contains(&Cord {
                                    x: x + dx,
                                    y: y + dy,
                                }) {
                                    neighbors += 1;
                                }
                            }
                        }
                        let should_be = (!is_alive && neighbors == BIRTH)
                            || (is_alive && (neighbors == SURVIVAL || neighbors == BIRTH));
                        if should_be {
                            new_cells.push(cord);
                        }
                    }
                }
                (chunk_cord, new_cells)
            })
            .collect();

        let mut new_chunks = HashMap::new();
        for (chunk_cord, cells) in next_cells {
            if cells.is_empty() {
                continue;
            }
            let chunk = new_chunks.entry(chunk_cord).or_insert_with(Chunk::new);
            for cord in cells {
                let (lx, ly) = Self::local_coords(cord.x, cord.y);
                chunk.set_local(lx, ly, true);
            }
        }
        self.chunks = new_chunks;
    }

    pub fn draw(&self, buf: &mut Buffer, rect: Rect, camera: &Camera) {
        if camera.zoom == 1 {
            self.draw_normal(buf, rect, camera);
        } else {
            self.draw_braille(buf, rect, camera);
        }
    }

    fn draw_normal(&self, buf: &mut Buffer, rect: Rect, camera: &Camera) {
        let left = camera.pos_x;
        let right = camera.pos_x + rect.width as i64;
        let top = camera.pos_y;
        let bottom = camera.pos_y + rect.height as i64;

        let chunk_left = left.div_euclid(CHUNK_SIZE as i64);
        let chunk_right = right.div_euclid(CHUNK_SIZE as i64);
        let chunk_top = top.div_euclid(CHUNK_SIZE as i64);
        let chunk_bottom = bottom.div_euclid(CHUNK_SIZE as i64);

        for cx in chunk_left..=chunk_right {
            for cy in chunk_top..=chunk_bottom {
                if let Some(chunk) = self.chunks.get(&ChunkCord {
                    chunk_x: cx,
                    chunk_y: cy,
                }) {
                    let offset_x = cx * CHUNK_SIZE as i64;
                    let offset_y = cy * CHUNK_SIZE as i64;
                    for (idx, &word) in chunk.bits.iter().enumerate() {
                        let mut bits = word;
                        let base_idx = idx * BITS_PER_U64;
                        while bits != 0 {
                            let t = bits.trailing_zeros() as usize;
                            let global_idx = base_idx + t;
                            let x = (global_idx % CHUNK_SIZE) as i64;
                            let y = (global_idx / CHUNK_SIZE) as i64;
                            let world_x = offset_x + x;
                            let world_y = offset_y + y;
                            if world_x >= left
                                && world_x < right
                                && world_y >= top
                                && world_y < bottom
                            {
                                if let Some((bx, by)) =
                                    camera.world_to_buffer(world_x, world_y, rect)
                                {
                                    buf.get_mut(bx, by).set_char('#').set_fg(Color::Green);
                                }
                            }
                            bits &= bits - 1;
                        }
                    }
                }
            }
        }
    }

    fn draw_braille(&self, buf: &mut Buffer, rect: Rect, camera: &Camera) {
        let cols = rect.width as usize;
        let rows = rect.height as usize;

        for screen_y in 0..rows {
            for screen_x in 0..cols {
                let world_x = camera.pos_x + (screen_x as i64) * 2;
                let world_y = camera.pos_y + (screen_y as i64) * 4;

                let mut bits = 0u8;
                for dx in 0..2 {
                    for dy in 0..4 {
                        if self.is_cell_alive(world_x + dx, world_y + dy) {
                            let bit = match (dx, dy) {
                                (0, 0) => 0x01,
                                (0, 1) => 0x02,
                                (0, 2) => 0x04,
                                (0, 3) => 0x40,
                                (1, 0) => 0x08,
                                (1, 1) => 0x10,
                                (1, 2) => 0x20,
                                (1, 3) => 0x80,
                                _ => 0,
                            };
                            bits |= bit;
                        }
                    }
                }
                let ch = if bits != 0 {
                    char::from_u32(0x2800 | bits as u32).unwrap()
                } else {
                    ' '
                };
                let buffer_x = rect.x + screen_x as u16;
                let buffer_y = rect.y + screen_y as u16;
                buf.get_mut(buffer_x, buffer_y)
                    .set_char(ch)
                    .set_fg(Color::Green);
            }
        }
    }
}

// ---------- Точка входа ----------
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let stdout = io::stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let mut stdout = io::stdout();
    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(
        stdout,
        crossterm::event::EnableMouseCapture,
        crossterm::terminal::EnterAlternateScreen
    )?;

    let mut world = World::new();
    let mut camera = Camera::new();

    let mut running = true;
    let mut last_tick = Instant::now();
    let mut paused = false;

    const INTERVALS_MS: [u64; 5] = [2000, 1000, 500, 250, 125];
    let mut speed_index = 2;
    let mut tick_interval = Duration::from_millis(INTERVALS_MS[speed_index]);

    while running {
        if !paused && last_tick.elapsed() >= tick_interval {
            world.tick();
            last_tick = Instant::now();
        }

        terminal.draw(|f| {
            let area = f.area();
            world.draw(f.buffer_mut(), area, &camera);
        })?;

        if event::poll(Duration::from_millis(16))? {
            match event::read()? {
                Event::Key(key) => {
                    let repeatable = key.code == KeyCode::Char('t')
                        || key.code == KeyCode::Char('l')
                        || key.code == KeyCode::Char('j')
                        || key.code == KeyCode::Char('+')
                        || key.code == KeyCode::Char('=')
                        || key.code == KeyCode::Char('-')
                        || key.code == KeyCode::Char(' ');
                    if repeatable && key.kind != event::KeyEventKind::Press {
                        continue;
                    }

                    match key.code {
                        KeyCode::Char('q') => running = false,
                        KeyCode::Char('t') => world.tick(),
                        KeyCode::Char(' ') => {
                            paused = !paused;
                            if !paused {
                                last_tick = Instant::now();
                            }
                        }
                        KeyCode::Char('l') => {
                            if speed_index > 0 {
                                speed_index -= 1;
                                tick_interval = Duration::from_millis(INTERVALS_MS[speed_index]);
                            }
                        }
                        KeyCode::Char('j') => {
                            if speed_index < INTERVALS_MS.len() - 1 {
                                speed_index += 1;
                                tick_interval = Duration::from_millis(INTERVALS_MS[speed_index]);
                            }
                        }
                        KeyCode::Char('+') | KeyCode::Char('=') => camera.zoom_in(),
                        KeyCode::Char('-') => camera.zoom_out(),
                        KeyCode::Left => {
                            camera.try_move(-1, 0);
                        }
                        KeyCode::Right => {
                            camera.try_move(1, 0);
                        }
                        KeyCode::Up => {
                            camera.try_move(0, -1);
                        }
                        KeyCode::Down => {
                            camera.try_move(0, 1);
                        }
                        _ => {}
                    }
                }
                Event::Mouse(me) => {
                    if let MouseEventKind::Down(btn) = me.kind {
                        let term_rect = terminal.size()?;
                        let rect = Rect::new(0, 0, term_rect.width, term_rect.height);
                        if camera.zoom == 1 {
                            let (world_x, world_y) =
                                camera.buffer_to_world(me.column, me.row, rect);
                            match btn {
                                MouseButton::Left => world.animate_cell(world_x, world_y),
                                MouseButton::Right => world.kill_cell(world_x, world_y),
                                _ => {}
                            }
                        } else {
                            let screen_col = me.column as usize;
                            let screen_row = me.row as usize;
                            let block_x = camera.pos_x + (screen_col as i64 - rect.x as i64) * 2;
                            let block_y = camera.pos_y + (screen_row as i64 - rect.y as i64) * 4;
                            match btn {
                                MouseButton::Left => world.animate_block(block_x, block_y),
                                MouseButton::Right => world.kill_block(block_x, block_y),
                                _ => {}
                            }
                        }
                    }
                }
                Event::Resize(w, h) => {
                    terminal.resize(Rect::new(0, 0, w, h))?;
                }
                _ => {}
            }
        }
    }

    crossterm::execute!(
        stdout,
        crossterm::event::DisableMouseCapture,
        crossterm::terminal::LeaveAlternateScreen
    )?;
    crossterm::terminal::disable_raw_mode()?;
    Ok(())
}