//! Conway's Game of Life с бесконечным миром и TUI-интерфейсом.
//! Два экрана: настройки и симуляция, переключение по Esc.
//! Все бинды настраиваются, правила симуляции и скорость сохраняются в CSV.

use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind, read};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

// ---------- Константы игры ----------
const CHUNK_SIZE: usize = 32;
const CHUNK_BITS: usize = CHUNK_SIZE * CHUNK_SIZE; // 1024
const BITS_PER_U64: usize = 64;
const U64_COUNT: usize = CHUNK_BITS / BITS_PER_U64; // 16

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
    survival: usize,
    birth: usize,
}

impl World {
    pub fn new(survival: usize, birth: usize) -> Self {
        Self {
            chunks: HashMap::new(),
            survival,
            birth,
        }
    }

    pub fn set_rules(&mut self, survival: usize, birth: usize) {
        self.survival = survival;
        self.birth = birth;
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
        let survival = self.survival;
        let birth = self.birth;

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
                        let should_be = (!is_alive && neighbors == birth)
                            || (is_alive && (neighbors == survival || neighbors == birth));
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

// ---------- Структуры для работы с настройками ----------
#[derive(Debug, Serialize, Deserialize)]
struct SettingsRecord {
    action_id: String,
    key_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct SimulationRecord {
    cells_to_alive: usize,
    cells_to_birth: usize,
    simulation_speed: usize,
}

// ---------- Состояние приложения ----------
#[derive(PartialEq, Clone)]
enum Screen {
    Settings,
    Simulation,
}

struct AppState {
    screen: Screen,
    simulations_settings: HashMap<String, usize>,
    error_message: String,
    key_bindings: HashMap<String, String>,
}

impl AppState {
    fn new() -> Self {
        let mut x = Self {
            screen: Screen::Settings,
            simulations_settings: HashMap::new(),
            error_message: String::new(),
            key_bindings: HashMap::new(),
        };
        if let Err(e) = x.load_last_saves() {
            x.error_message = format!("Ошибка загрузки: {}", e);
        }
        x
    }

    fn load_last_saves(&mut self) -> std::io::Result<()> {
        let last_saves = last_saves_path();
        if !last_saves.exists() {
            self.create_default_files()?;
            return Ok(());
        }

        let mut reader = csv::Reader::from_path(&last_saves)?;
        if let Ok(record) = reader.headers() {
            let settings_path = PathBuf::from(&record[0]);
            let sim_path = PathBuf::from(&record[1]);
            self.load_settings_from_file(&settings_path)?;
            self.load_simulation_from_file(&sim_path)?;
        }
        Ok(())
    }

    fn create_default_files(&mut self) -> std::io::Result<()> {
        std::fs::create_dir_all(default_settings_dir())?;
        std::fs::create_dir_all(default_simulation_dir())?;

        let settings_path = default_settings_path();
        let sim_path = default_simulation_path();

        if !settings_path.exists() {
            self.key_bindings = default_key_bindings();
            self.save_settings_to_file(&settings_path)?;
        } else {
            self.load_settings_from_file(&settings_path)?;
        }

        if !sim_path.exists() {
            self.simulations_settings = default_simulation_settings();
            self.save_simulation_to_file(&sim_path)?;
        } else {
            self.load_simulation_from_file(&sim_path)?;
        }

        let mut writer = csv::Writer::from_path(last_saves_path())?;
        writer.write_record(&[settings_path.to_str().unwrap(), sim_path.to_str().unwrap()])?;
        writer.flush()?;
        Ok(())
    }

    fn save_current_settings(&self) -> std::io::Result<()> {
        self.save_settings_to_file(&current_settings_path())?;
        self.save_simulation_to_file(&current_simulation_path())?;
        Ok(())
    }

    fn import_settings_from_file(&mut self, path: PathBuf) -> std::io::Result<()> {
        let mut temp_bindings = HashMap::new();
        let mut reader = csv::Reader::from_path(&path)?;
        for result in reader.deserialize() {
            let record: SettingsRecord =
                result.map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            temp_bindings.insert(record.action_id, record.key_name);
        }
        let new_name = format!("imported_{}", path.file_name().unwrap().to_string_lossy());
        let new_path = default_settings_dir().join(new_name);
        AppState::update_last_saves(&new_path, &default_simulation_path())?;
        std::fs::copy(&path, &new_path)?;
        self.key_bindings = temp_bindings;
        self.save_current_settings()?;
        Ok(())
    }

    fn load_settings_from_file(&mut self, path: &Path) -> std::io::Result<()> {
        let mut reader = csv::Reader::from_path(path)?;
        for result in reader.deserialize() {
            let record: SettingsRecord =
                result.map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            self.key_bindings.insert(record.action_id, record.key_name);
        }
        Ok(())
    }

    fn load_simulation_from_file(&mut self, path: &Path) -> std::io::Result<()> {
        let mut reader = csv::Reader::from_path(path)?;
        if let Some(result) = reader.deserialize().next() {
            let record: SimulationRecord =
                result.map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            self.simulations_settings
                .insert("cells_to_alive".to_string(), record.cells_to_alive);
            self.simulations_settings
                .insert("cells_to_birth".to_string(), record.cells_to_birth);
            self.simulations_settings
                .insert("simulation_speed".to_string(), record.simulation_speed);
        }
        Ok(())
    }

    fn save_settings_to_file(&self, path: &Path) -> std::io::Result<()> {
        let mut writer = csv::Writer::from_path(path)?;
        for (action_id, key_name) in &self.key_bindings {
            writer.serialize(SettingsRecord {
                action_id: action_id.clone(),
                key_name: key_name.clone(),
            })?;
        }
        writer.flush()?;
        Ok(())
    }

    fn save_simulation_to_file(&self, path: &Path) -> std::io::Result<()> {
        let cells_to_alive = *self
            .simulations_settings
            .get("cells_to_alive")
            .unwrap_or(&1);
        let cells_to_birth = *self
            .simulations_settings
            .get("cells_to_birth")
            .unwrap_or(&3);
        let simulation_speed = *self
            .simulations_settings
            .get("simulation_speed")
            .unwrap_or(&1);
        let record = SimulationRecord {
            cells_to_alive,
            cells_to_birth,
            simulation_speed,
        };
        let mut writer = csv::Writer::from_path(path)?;
        writer.serialize(record)?;
        writer.flush()?;
        Ok(())
    }

    fn update_last_saves(settings_path: &Path, simulation_path: &Path) -> std::io::Result<()> {
        let mut writer = csv::Writer::from_path(last_saves_path())?;
        writer.write_record(&[
            settings_path.to_str().unwrap(),
            simulation_path.to_str().unwrap(),
        ])?;
        writer.flush()?;
        Ok(())
    }
}

// ---------- Компоненты TUI (настройки) ----------
trait Component {
    fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        focus_stack: Vec<usize>,
        focus_index: usize,
        has_focused_parent: bool,
        app_state: Rc<RefCell<AppState>>,
    );
    fn handle_event(&mut self, ev: &Event, app_state: Rc<RefCell<AppState>>) -> Feedback;
    fn get_child(&mut self, index: usize) -> Option<&mut Box<dyn Component>>;
    fn get_children_count(&mut self) -> usize;
    fn can_be_in_focus(&self) -> AbilityToFocus;
    fn get_focusable_indexes(&mut self) -> Vec<usize>;
}

enum Feedback {
    Esc,
    None,
}

#[derive(PartialEq)]
enum AbilityToFocus {
    CanBeInFocus,
    CantBeInFocus,
}

struct Container {
    title: String,
    direction: Direction,
    can_be_in_focus: bool,
    children: Vec<Box<dyn Component>>,
    borders: bool,
}

impl Container {
    fn new() -> Self {
        Self {
            title: String::new(),
            direction: Direction::Horizontal,
            can_be_in_focus: true,
            children: vec![],
            borders: false,
        }
    }

    fn add_child(&mut self, child: Box<dyn Component>) {
        self.children.push(child);
    }
}

impl Component for Container {
    fn handle_event(&mut self, _ev: &Event, _app_state: Rc<RefCell<AppState>>) -> Feedback {
        Feedback::None
    }

    fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        focus_stack: Vec<usize>,
        focus_index: usize,
        has_focused_parent: bool,
        app_state: Rc<RefCell<AppState>>,
    ) {
        let is_focused =
            !focus_stack.is_empty() && focus_stack[0] == focus_index && has_focused_parent;
        let borders = if self.borders {
            Borders::ALL
        } else {
            Borders::NONE
        };
        let title = if is_focused {
            self.title.clone()
        } else {
            String::new()
        };
        let focus_style = if is_focused && focus_stack.len() == 1 {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::Black)
        };
        let focus_stack_to_children = if !focus_stack.is_empty() {
            focus_stack[1..].to_vec()
        } else {
            Vec::new()
        };
        let container = Block::default()
            .borders(borders)
            .border_style(focus_style)
            .title(title);
        frame.render_widget(&container, area);
        if self.children.is_empty() {
            return;
        }
        let inner_area = container.inner(area);
        let layout = Layout::default()
            .direction(self.direction)
            .constraints(std::iter::repeat(Constraint::Fill(1)).take(self.children.len()))
            .split(inner_area);
        for i in 0..self.children.len() {
            self.children[i].render(
                frame,
                layout[i],
                focus_stack_to_children.clone(),
                i,
                is_focused,
                Rc::clone(&app_state),
            );
        }
    }

    fn get_child(&mut self, index: usize) -> Option<&mut Box<dyn Component>> {
        self.children.get_mut(index)
    }

    fn get_focusable_indexes(&mut self) -> Vec<usize> {
        self.children
            .iter()
            .enumerate()
            .filter(|(_, c)| c.can_be_in_focus() != AbilityToFocus::CantBeInFocus)
            .map(|(i, _)| i)
            .collect()
    }

    fn get_children_count(&mut self) -> usize {
        self.children
            .iter()
            .filter(|c| c.can_be_in_focus() != AbilityToFocus::CantBeInFocus)
            .count()
    }

    fn can_be_in_focus(&self) -> AbilityToFocus {
        if self.can_be_in_focus {
            AbilityToFocus::CanBeInFocus
        } else {
            AbilityToFocus::CantBeInFocus
        }
    }
}

struct Label {
    text: String,
    border: bool,
}

impl Component for Label {
    fn handle_event(&mut self, _ev: &Event, _app_state: Rc<RefCell<AppState>>) -> Feedback {
        Feedback::None
    }

    fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        _focus_stack: Vec<usize>,
        _focus_index: usize,
        _has_focused_parent: bool,
        _app_state: Rc<RefCell<AppState>>,
    ) {
        let borders = if self.border {
            Borders::ALL
        } else {
            Borders::NONE
        };
        let container = Block::default()
            .borders(borders)
            .border_style(Style::default().fg(Color::DarkGray))
            .border_type(BorderType::LightDoubleDashed);
        frame.render_widget(&container, area);
        let inner_area = container.inner(area);
        let label = Paragraph::new(self.text.clone())
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: true });
        frame.render_widget(label, inner_area);
    }

    fn get_child(&mut self, _index: usize) -> Option<&mut Box<dyn Component>> {
        None
    }

    fn get_children_count(&mut self) -> usize {
        0
    }

    fn can_be_in_focus(&self) -> AbilityToFocus {
        AbilityToFocus::CantBeInFocus
    }

    fn get_focusable_indexes(&mut self) -> Vec<usize> {
        vec![]
    }
}

struct Input {
    id: String,
    title: String,
    value: String,
    cursor: usize,
    editing: bool,
    border: bool,
}

impl Input {
    fn new(id: String, app_state: Rc<RefCell<AppState>>) -> Self {
        let value = {
            let st = app_state.borrow();
            st.simulations_settings
                .get(&id)
                .map(|v| v.to_string())
                .unwrap_or_else(|| "0".to_string())
        };
        Self {
            id,
            title: String::new(),
            value: value.clone(),
            cursor: value.len(),
            editing: false,
            border: true,
        }
    }

    fn get_value(&self) -> i32 {
        self.value.trim().parse::<i32>().unwrap_or(0)
    }

    fn insert_char(&mut self, ch: char, app_state: Rc<RefCell<AppState>>) {
        self.value.insert(self.cursor, ch);
        self.cursor += 1;
        let mut st = app_state.borrow_mut();
        st.simulations_settings
            .insert(self.id.clone(), self.get_value() as usize);
    }

    fn delete_char(&mut self, app_state: Rc<RefCell<AppState>>) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.value.remove(self.cursor);
            let mut st = app_state.borrow_mut();
            st.simulations_settings
                .insert(self.id.clone(), self.get_value() as usize);
        }
    }

    fn shift_left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    fn shift_right(&mut self) {
        if self.cursor < self.value.len() {
            self.cursor += 1;
        }
    }
}

impl Component for Input {
    fn get_focusable_indexes(&mut self) -> Vec<usize> {
        vec![]
    }

    fn get_child(&mut self, _index: usize) -> Option<&mut Box<dyn Component>> {
        None
    }

    fn get_children_count(&mut self) -> usize {
        1
    }

    fn can_be_in_focus(&self) -> AbilityToFocus {
        AbilityToFocus::CanBeInFocus
    }

    fn handle_event(&mut self, ev: &Event, app_state: Rc<RefCell<AppState>>) -> Feedback {
        self.editing = true;
        if let Event::Key(key) = ev {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Char(ch) if ch.is_ascii_digit() => {
                        self.insert_char(ch, app_state);
                    }
                    KeyCode::Backspace => {
                        self.delete_char(app_state);
                    }
                    KeyCode::Left => {
                        self.shift_left();
                    }
                    KeyCode::Right => {
                        self.shift_right();
                    }
                    KeyCode::Esc => {
                        self.editing = false;
                    }
                    _ => {
                        app_state.borrow_mut().error_message = "разрешены только цифры".to_string();
                    }
                }
            }
        }
        Feedback::None
    }

    fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        focus_stack: Vec<usize>,
        focus_index: usize,
        has_focused_parent: bool,
        _app_state: Rc<RefCell<AppState>>,
    ) {
        let is_focused =
            !focus_stack.is_empty() && focus_stack[0] == focus_index && has_focused_parent;
        let is_entered = self.editing;
        let borders = if self.border {
            Borders::ALL
        } else {
            Borders::NONE
        };
        let title = self.title.clone();
        let border_style = if is_focused && focus_stack.len() == 1 || is_entered {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::White)
        };
        let container = Block::default()
            .borders(borders)
            .border_style(border_style)
            .title(title);
        frame.render_widget(&container, area);
        let inner_area = container.inner(area);
        let input_spans = {
            let input = self.value.clone();
            let pos = self.cursor;
            let chars: Vec<char> = input.chars().collect();
            let mut spans = Vec::new();
            if pos > 0 {
                let before: String = chars[..pos].iter().collect();
                spans.push(Span::raw(before));
            }
            if pos < chars.len() {
                let c = chars[pos];
                spans.push(Span::styled(c.to_string(), Style::default().reversed()));
            } else {
                spans.push(Span::styled(" ", Style::default().reversed()));
            }
            if pos + 1 < chars.len() {
                let after: String = chars[pos + 1..].iter().collect();
                spans.push(Span::raw(after));
            }
            spans
        };
        let text = if is_entered {
            Line::from(input_spans)
        } else {
            Line::from(self.value.clone())
        };
        let input = Paragraph::new(text)
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: true });
        frame.render_widget(input, inner_area);
    }
}

struct Button {
    callback: Option<Box<dyn Fn(Rc<RefCell<AppState>>)>>,
}

impl Button {
    fn new(callback: impl Fn(Rc<RefCell<AppState>>) + 'static) -> Self {
        Self {
            callback: Some(Box::new(callback)),
        }
    }
}

impl Component for Button {
    fn get_child(&mut self, _index: usize) -> Option<&mut Box<dyn Component>> {
        None
    }

    fn get_children_count(&mut self) -> usize {
        1
    }

    fn can_be_in_focus(&self) -> AbilityToFocus {
        AbilityToFocus::CanBeInFocus
    }

    fn get_focusable_indexes(&mut self) -> Vec<usize> {
        vec![]
    }

    fn handle_event(&mut self, ev: &Event, app_state: Rc<RefCell<AppState>>) -> Feedback {
        if let Event::Key(key) = ev {
            if key.kind == KeyEventKind::Press && key.code == KeyCode::Enter {
                if let Some(callback) = &self.callback {
                    callback(app_state);
                    return Feedback::Esc;
                }
            }
        }
        Feedback::None
    }

    fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        focus_stack: Vec<usize>,
        focus_index: usize,
        has_focused_parent: bool,
        _app_state: Rc<RefCell<AppState>>,
    ) {
        let is_focused =
            !focus_stack.is_empty() && focus_stack[0] == focus_index && has_focused_parent;
        let color = if is_focused {
            Color::Yellow
        } else {
            Color::White
        };
        let container = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(color));
        frame.render_widget(&container, area);
    }
}

struct KeybindInput {
    action_id: String,
    label: String,
    editing: bool,
}

impl KeybindInput {
    fn new(action_id: String, label: String) -> Self {
        Self {
            action_id,
            label,
            editing: false,
        }
    }

    fn current_binding(&self, app_state: &Rc<RefCell<AppState>>) -> String {
        app_state
            .borrow()
            .key_bindings
            .get(&self.action_id)
            .cloned()
            .unwrap_or_else(|| "?".to_string())
    }

    fn set_binding(&self, key_name: String, app_state: &Rc<RefCell<AppState>>) {
        app_state
            .borrow_mut()
            .key_bindings
            .insert(self.action_id.clone(), key_name);
    }
}

impl Component for KeybindInput {
    fn can_be_in_focus(&self) -> AbilityToFocus {
        AbilityToFocus::CanBeInFocus
    }

    fn get_child(&mut self, _index: usize) -> Option<&mut Box<dyn Component>> {
        None
    }

    fn get_children_count(&mut self) -> usize {
        1
    }

    fn get_focusable_indexes(&mut self) -> Vec<usize> {
        vec![]
    }

    fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        focus_stack: Vec<usize>,
        focus_index: usize,
        has_focused_parent: bool,
        app_state: Rc<RefCell<AppState>>,
    ) {
        let is_focused =
            !focus_stack.is_empty() && focus_stack[0] == focus_index && has_focused_parent;
        let is_editing = self.editing;
        let borders = if is_focused || is_editing {
            Borders::ALL
        } else {
            Borders::NONE
        };
        let border_style = if is_focused || is_editing {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::Black)
        };
        let block = Block::default().borders(borders).border_style(border_style);
        frame.render_widget(&block, area);
        let inner = block.inner(area);
        let value = if is_editing {
            "press key".to_string()
        } else {
            self.current_binding(&app_state)
        };
        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Fill(1), Constraint::Fill(1)])
            .split(inner);
        let display_label = Paragraph::new(self.label.clone())
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: true });
        let display_value = Paragraph::new(value)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });
        frame.render_widget(display_label, layout[0]);
        frame.render_widget(display_value, layout[1]);
    }

    fn handle_event(&mut self, ev: &Event, app_state: Rc<RefCell<AppState>>) -> Feedback {
        if let Event::Key(key) = ev {
            if key.kind == KeyEventKind::Press {
                if self.editing {
                    let canonical = canonical_key_name(key.code);
                    self.set_binding(canonical.clone(), &app_state);
                    self.editing = false;
                    app_state.borrow_mut().error_message.clear();
                    return Feedback::Esc;
                } else if key.code == KeyCode::Enter {
                    self.editing = true;
                }
            }
        }
        Feedback::None
    }
}

// ---------- Основное приложение, управляющее экранами ----------
struct App {
    app_state: Rc<RefCell<AppState>>,
    settings_root: Box<dyn Component>,
    settings_focus_stack: Vec<usize>,
    world: World,
    camera: Camera,
    paused: bool,
    last_tick: Instant,
    tick_interval: Duration,
    speed_index: usize,
    glider_placed: bool,
}

impl App {
    fn new(app_state: Rc<RefCell<AppState>>) -> Self {
        let survival = app_state
            .borrow()
            .simulations_settings
            .get("cells_to_alive")
            .copied()
            .unwrap_or(1);
        let birth = app_state
            .borrow()
            .simulations_settings
            .get("cells_to_birth")
            .copied()
            .unwrap_or(3);
        let speed = app_state
            .borrow()
            .simulations_settings
            .get("simulation_speed")
            .copied()
            .unwrap_or(1);
        let speed_index = match speed {
            1 => 2, // 500 ms
            2 => 3, // 250 ms
            3 => 4, // 125 ms
            4 => 1, // 1000 ms
            5 => 0, // 2000 ms
            _ => 2,
        };
        let intervals = [2000, 1000, 500, 250, 125];
        let tick_interval = Duration::from_millis(intervals[speed_index]);

        Self {
            settings_root: Self::build_settings_root(app_state.clone()),
            settings_focus_stack: vec![0, 0],
            app_state: app_state.clone(),
            world: World::new(survival, birth),
            camera: Camera::new(),
            paused: false,
            last_tick: Instant::now(),
            tick_interval,
            speed_index,
            glider_placed: false,
        }
    }

    fn build_settings_root(app_state: Rc<RefCell<AppState>>) -> Box<dyn Component> {
        let mut root_container = Container::new();
        // Колонка 1 – настройки мира
        let mut col1 = Container {
            title: "мир".to_string(),
            direction: Direction::Vertical,
            can_be_in_focus: true,
            borders: true,
            children: vec![],
        };
        let mut cell_count = Container {
            title: String::new(),
            direction: Direction::Vertical,
            can_be_in_focus: true,
            borders: true,
            children: vec![],
        };
        cell_count.add_child(Box::new(Label {
            text: "Количество клеток:".to_string(),
            border: true,
        }));
        let mut input_alive = Input::new("cells_to_alive".to_string(), app_state.clone());
        input_alive.title = "для выживания:".to_string();
        cell_count.add_child(Box::new(input_alive));
        let mut input_birth = Input::new("cells_to_birth".to_string(), app_state.clone());
        input_birth.title = "для рождения:".to_string();
        cell_count.add_child(Box::new(input_birth));
        col1.add_child(Box::new(cell_count));

        let mut sim_speed = Container {
            title: String::new(),
            direction: Direction::Vertical,
            can_be_in_focus: true,
            borders: true,
            children: vec![],
        };
        sim_speed.add_child(Box::new(Label {
            text: "Скорость симуляции:".to_string(),
            border: true,
        }));
        sim_speed.add_child(Box::new(Input::new(
            "simulation_speed".to_string(),
            app_state.clone(),
        )));
        col1.add_child(Box::new(sim_speed));

        let mut buttons = Container {
            title: String::new(),
            direction: Direction::Horizontal,
            can_be_in_focus: true,
            borders: true,
            children: vec![],
        };
        let mut col_actions = Container {
            title: String::new(),
            direction: Direction::Vertical,
            can_be_in_focus: false,
            borders: false,
            children: vec![],
        };
        col_actions.add_child(Box::new(Label {
            text: "действие".to_string(),
            border: true,
        }));
        col_actions.add_child(Box::new(Label {
            text: "сохранить".to_string(),
            border: true,
        }));
        col_actions.add_child(Box::new(Label {
            text: "импорт".to_string(),
            border: true,
        }));
        col_actions.add_child(Box::new(Label {
            text: "экспорт".to_string(),
            border: true,
        }));
        buttons.add_child(Box::new(col_actions));

        let mut col_settings = Container {
            title: String::new(),
            direction: Direction::Vertical,
            can_be_in_focus: true,
            borders: true,
            children: vec![],
        };
        col_settings.add_child(Box::new(Label {
            text: "настройки".to_string(),
            border: true,
        }));
        col_settings.add_child(Box::new(Button::new(|state| {
            if let Err(e) = state.borrow().save_current_settings() {
                state.borrow_mut().error_message = format!("Ошибка сохранения: {}", e);
            } else {
                state.borrow_mut().error_message = "Настройки сохранены".to_string();
            }
        })));
        col_settings.add_child(Box::new(Button::new(|state| {
            if let Some(path) = rfd::FileDialog::new()
                .set_title("Выберите файл с настройками клавиш")
                .add_filter("CSV", &["csv"])
                .pick_file()
            {
                let mut st = state.borrow_mut();
                if let Err(e) = st.import_settings_from_file(path) {
                    st.error_message = format!("Ошибка импорта: {}", e);
                } else {
                    st.error_message = "Настройки импортированы".to_string();
                }
            }
        })));
        col_settings.add_child(Box::new(Button::new(|state| {
            if let Some(path) = rfd::FileDialog::new()
                .set_title("Сохранить настройки клавиш")
                .add_filter("CSV", &["csv"])
                .save_file()
            {
                let _ = state.borrow().save_settings_to_file(&path);
                let sim_path = default_simulation_path();
                let _ = AppState::update_last_saves(&path, &sim_path);
                state.borrow_mut().error_message = format!("Сохранено в {}", path.display());
            }
        })));
        buttons.add_child(Box::new(col_settings));

        let mut col_sim = Container {
            title: String::new(),
            direction: Direction::Vertical,
            can_be_in_focus: true,
            borders: true,
            children: vec![],
        };
        col_sim.add_child(Box::new(Label {
            text: "симуляция".to_string(),
            border: true,
        }));
        col_sim.add_child(Box::new(Button::new(|state| {
            state.borrow_mut().screen = Screen::Simulation;
        })));
        col_sim.add_child(Box::new(Button::new(|_| {})));
        col_sim.add_child(Box::new(Button::new(|_| {})));
        buttons.add_child(Box::new(col_sim));
        col1.add_child(Box::new(buttons));
        root_container.add_child(Box::new(col1));

        // Колонка 2 – настройка биндов
        let mut col2 = Container {
            title: String::new(),
            direction: Direction::Vertical,
            can_be_in_focus: true,
            borders: true,
            children: vec![],
        };
        let actions = vec![
            ("pause", "пауза"),
            ("right", "→"),
            ("left", "←"),
            ("up", "↑"),
            ("down", "↓"),
            ("zoom_in", "приблизить"),
            ("zoom_out", "отдалить"),
            ("speed_up", "ускорить"),
            ("speed_down", "замедлить"),
            ("step", "совершить одну итерацию"),
        ];
        for (id, label) in actions {
            col2.add_child(Box::new(KeybindInput::new(
                id.to_string(),
                label.to_string(),
            )));
        }
        root_container.add_child(Box::new(col2));
        // третья колонка пустая
        root_container.add_child(Box::new(Container::new()));
        Box::new(root_container)
    }

    fn update_simulation_from_settings(&mut self) {
        let st = self.app_state.borrow();
        let survival = st
            .simulations_settings
            .get("cells_to_alive")
            .copied()
            .unwrap_or(1);
        let birth = st
            .simulations_settings
            .get("cells_to_birth")
            .copied()
            .unwrap_or(3);
        let speed = st
            .simulations_settings
            .get("simulation_speed")
            .copied()
            .unwrap_or(1);
        self.world.set_rules(survival, birth);
        let intervals = [2000, 1000, 500, 250, 125];
        let new_index = match speed {
            1 => 2,
            2 => 3,
            3 => 4,
            4 => 1,
            5 => 0,
            _ => 2,
        };
        self.speed_index = new_index;
        self.tick_interval = Duration::from_millis(intervals[self.speed_index]);
        if !self.glider_placed && self.world.chunks.is_empty() {
            self.world.animate_cell(10, 10);
            self.world.animate_cell(11, 10);
            self.world.animate_cell(12, 10);
            self.world.animate_cell(10, 11);
            self.world.animate_cell(11, 12);
            self.glider_placed = true;
        }
    }

    fn handle_simulation_input(&mut self, ev: &Event) {
        if let Event::Key(key) = ev {
            if key.kind == KeyEventKind::Press {
                let key_name = canonical_key_name(key.code);
                let st = self.app_state.borrow();
                let action = st.key_bindings.iter().find_map(|(act, name)| {
                    if name == &key_name {
                        Some(act.clone())
                    } else {
                        None
                    }
                });
                drop(st);
                match action.as_deref() {
                    Some("pause") => {
                        self.paused = !self.paused;
                        if !self.paused {
                            self.last_tick = Instant::now();
                        }
                    }
                    Some("step") => {
                        self.world.tick();
                    }
                    Some("right") => {
                        self.camera.try_move(1, 0);
                    }
                    Some("left") => {
                        self.camera.try_move(-1, 0);
                    }
                    Some("up") => {
                        self.camera.try_move(0, -1);
                    }
                    Some("down") => {
                        self.camera.try_move(0, 1);
                    }
                    Some("zoom_in") => {
                        self.camera.zoom_in();
                    }
                    Some("zoom_out") => {
                        self.camera.zoom_out();
                    }
                    Some("speed_up") => {
                        if self.speed_index < 4 {
                            self.speed_index += 1;
                            let intervals = [2000, 1000, 500, 250, 125];
                            self.tick_interval = Duration::from_millis(intervals[self.speed_index]);
                            let new_speed = match self.speed_index {
                                0 => 5,
                                1 => 4,
                                2 => 1,
                                3 => 2,
                                4 => 3,
                                _ => 1,
                            };
                            self.app_state
                                .borrow_mut()
                                .simulations_settings
                                .insert("simulation_speed".to_string(), new_speed);
                        }
                    }
                    Some("speed_down") => {
                        if self.speed_index > 0 {
                            self.speed_index -= 1;
                            let intervals = [2000, 1000, 500, 250, 125];
                            self.tick_interval = Duration::from_millis(intervals[self.speed_index]);
                            let new_speed = match self.speed_index {
                                0 => 5,
                                1 => 4,
                                2 => 1,
                                3 => 2,
                                4 => 3,
                                _ => 1,
                            };
                            self.app_state
                                .borrow_mut()
                                .simulations_settings
                                .insert("simulation_speed".to_string(), new_speed);
                        }
                    }
                    _ => {}
                }
            }
        } else if let Event::Mouse(me) = ev {
            if let MouseEventKind::Down(btn) = me.kind {
                let (width, height) = crossterm::terminal::size().unwrap_or((80, 24));
                let rect = Rect::new(0, 0, width, height);
                if self.camera.zoom == 1 {
                    let (world_x, world_y) = self.camera.buffer_to_world(me.column, me.row, rect);
                    match btn {
                        MouseButton::Left => self.world.animate_cell(world_x, world_y),
                        MouseButton::Right => self.world.kill_cell(world_x, world_y),
                        _ => {}
                    }
                } else {
                    let screen_col = me.column as usize;
                    let screen_row = me.row as usize;
                    let block_x = self.camera.pos_x + (screen_col as i64) * 2;
                    let block_y = self.camera.pos_y + (screen_row as i64) * 4;
                    match btn {
                        MouseButton::Left => self.world.animate_block(block_x, block_y),
                        MouseButton::Right => self.world.kill_block(block_x, block_y),
                        _ => {}
                    }
                }
            }
        }
    }

    fn render_simulation(&self, frame: &mut Frame, area: Rect) {
        self.world.draw(frame.buffer_mut(), area, &self.camera);
        let info = format!(
            "{} | speed: {}ms | zoom: {}",
            if self.paused { "PAUSED" } else { "RUNNING" },
            self.tick_interval.as_millis(),
            self.camera.zoom
        );
        let info_style = Style::default().fg(Color::Cyan);
        let info_widget = Paragraph::new(info)
            .style(info_style)
            .alignment(Alignment::Left);
        let info_area = Rect::new(0, 0, area.width, 1);
        frame.render_widget(info_widget, info_area);
    }

    fn render_settings(&mut self, frame: &mut Frame, area: Rect) {
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Fill(1), Constraint::Length(3)])
            .split(area);
        let tips_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Fill(1), Constraint::Fill(1)])
            .split(layout[1]);
        let tip = Paragraph::new("esc - назад |  ↑←→↓ - перемещение | enter - подтвердить")
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });
        let err_msg = {
            let st = self.app_state.borrow();
            st.error_message.clone()
        };
        let error_message = Paragraph::new(err_msg)
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::Yellow))
            .wrap(Wrap { trim: true });
        self.settings_root.render(
            frame,
            layout[0],
            self.settings_focus_stack.clone(),
            0,
            true,
            Rc::clone(&self.app_state),
        );
        frame.render_widget(tip, tips_layout[0]);
        frame.render_widget(error_message, tips_layout[1]);
    }
}

// ---------- Вспомогательные функции ----------
fn canonical_key_name(key: KeyCode) -> String {
    match key {
        KeyCode::Char(c) => {
            let lower = c.to_ascii_lowercase();
            match lower {
                'а'..='я' | 'ё' => {
                    let s = match c {
                        'й' => "Q",
                        'ц' => "W",
                        'у' => "E",
                        'к' => "R",
                        'е' => "T",
                        'н' => "Y",
                        'г' => "U",
                        'ш' => "I",
                        'щ' => "O",
                        'з' => "P",
                        'х' => "{",
                        'ъ' => "}",
                        'ф' => "A",
                        'ы' => "S",
                        'в' => "D",
                        'а' => "F",
                        'п' => "G",
                        'р' => "H",
                        'о' => "J",
                        'л' => "K",
                        'д' => "L",
                        'ж' => ":",
                        'э' => "'",
                        'я' => "Z",
                        'ч' => "X",
                        'с' => "C",
                        'м' => "V",
                        'и' => "B",
                        'т' => "N",
                        'ь' => "M",
                        'б' => "<",
                        'ю' => ">",
                        'ё' => "~",
                        _ => &c.to_uppercase().to_string(),
                    };
                    s.to_string()
                }
                ' ' => "Space".to_string(),
                _ => c.to_uppercase().to_string(),
            }
        }
        KeyCode::Enter => "Enter".to_string(),
        KeyCode::Left => "Left".to_string(),
        KeyCode::Right => "Right".to_string(),
        KeyCode::Up => "Up".to_string(),
        KeyCode::Down => "Down".to_string(),
        KeyCode::Backspace => "Backspace".to_string(),
        KeyCode::Tab => "Tab".to_string(),
        KeyCode::Esc => "Esc".to_string(),
        KeyCode::Delete => "Delete".to_string(),
        KeyCode::Home => "Home".to_string(),
        KeyCode::End => "End".to_string(),
        KeyCode::PageUp => "PageUp".to_string(),
        KeyCode::PageDown => "PageDown".to_string(),
        KeyCode::Insert => "Insert".to_string(),
        _ => "?".to_string(),
    }
}

fn default_saves_dir() -> PathBuf {
    let exe_path = std::env::current_exe().unwrap_or(PathBuf::from("."));
    let exe_dir = exe_path.parent().unwrap_or(&exe_path);
    exe_dir.join("saves")
}

fn default_settings_dir() -> PathBuf {
    default_saves_dir().join("settings")
}

fn default_simulation_dir() -> PathBuf {
    default_saves_dir().join("simulations")
}

fn current_simulation_path() -> PathBuf {
    let last_saves = last_saves_path();
    let mut reader = csv::Reader::from_path(&last_saves).unwrap();
    if let Ok(record) = reader.headers() {
        PathBuf::from(&record[1])
    } else {
        default_simulation_path()
    }
}

fn current_settings_path() -> PathBuf {
    let last_saves = last_saves_path();
    let mut reader = csv::Reader::from_path(&last_saves).unwrap();
    if let Ok(record) = reader.headers() {
        PathBuf::from(&record[0])
    } else {
        default_settings_path()
    }
}

fn default_settings_path() -> PathBuf {
    default_settings_dir().join("default_settings.csv")
}

fn default_simulation_path() -> PathBuf {
    default_simulation_dir().join("default_simulation.csv")
}

fn last_saves_path() -> PathBuf {
    default_saves_dir().join("last_saves.csv")
}

fn default_key_bindings() -> HashMap<String, String> {
    let mut map = HashMap::new();
    map.insert("pause".to_string(), "Space".to_string());
    map.insert("right".to_string(), "Right".to_string());
    map.insert("left".to_string(), "Left".to_string());
    map.insert("up".to_string(), "Up".to_string());
    map.insert("down".to_string(), "Down".to_string());
    map.insert("zoom_in".to_string(), "+".to_string());
    map.insert("zoom_out".to_string(), "-".to_string());
    map.insert("speed_up".to_string(), "L".to_string());
    map.insert("speed_down".to_string(), "J".to_string());
    map.insert("step".to_string(), "T".to_string());
    map
}

fn default_simulation_settings() -> HashMap<String, usize> {
    let mut map = HashMap::new();
    map.insert("cells_to_alive".to_string(), 1);
    map.insert("cells_to_birth".to_string(), 3);
    map.insert("simulation_speed".to_string(), 1);
    map
}

// ---------- Точка входа ----------
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let mut terminal = ratatui::init();
    let app_state = Rc::new(RefCell::new(AppState::new()));
    let mut app = App::new(app_state.clone());

    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(
        io::stdout(),
        crossterm::event::EnableMouseCapture,
        crossterm::terminal::EnterAlternateScreen
    )?;

    loop {
        let screen = app.app_state.borrow().screen.clone();

        if screen == Screen::Simulation
            && !app.paused
            && app.last_tick.elapsed() >= app.tick_interval
        {
            app.world.tick();
            app.last_tick = Instant::now();
        }

        terminal.draw(|f| {
            let area = f.area();
            match screen {
                Screen::Settings => app.render_settings(f, area),
                Screen::Simulation => app.render_simulation(f, area),
            }
        })?;

        if event::poll(Duration::from_millis(16))? {
            let ev = read()?;
            if let Event::Key(key) = &ev {
                if key.kind == KeyEventKind::Press && key.code == KeyCode::Char('q') {
                    break;
                }
            }
            match screen {
                Screen::Settings => {
                    if let Event::Key(key) = &ev {
                        if key.kind == KeyEventKind::Press {
                            match key.code {
                                KeyCode::Left | KeyCode::Up => {
                                    let focus_stack = app.settings_focus_stack.clone();
                                    let parent_slice =
                                        &focus_stack[..focus_stack.len().saturating_sub(1)];
                                    if let Some(parent) =
                                        app.settings_root.get_child(parent_slice[0])
                                    {
                                        let siblings = parent.get_children_count();
                                        if siblings > 1 {
                                            let focusable = parent.get_focusable_indexes();
                                            if let Some(last) = app.settings_focus_stack.last_mut()
                                            {
                                                let current_pos = focusable
                                                    .iter()
                                                    .position(|&ri| ri == *last)
                                                    .unwrap_or(0)
                                                    as isize;
                                                let new_pos = (current_pos - 1)
                                                    .rem_euclid(siblings as isize)
                                                    as usize;
                                                *last = focusable[new_pos];
                                            }
                                        }
                                    }
                                }
                                KeyCode::Down | KeyCode::Right => {
                                    let focus_stack = app.settings_focus_stack.clone();
                                    let parent_slice =
                                        &focus_stack[..focus_stack.len().saturating_sub(1)];
                                    if let Some(parent) =
                                        app.settings_root.get_child(parent_slice[0])
                                    {
                                        let siblings = parent.get_children_count();
                                        if siblings > 1 {
                                            let focusable = parent.get_focusable_indexes();
                                            if let Some(last) = app.settings_focus_stack.last_mut()
                                            {
                                                let current_pos = focusable
                                                    .iter()
                                                    .position(|&ri| ri == *last)
                                                    .unwrap_or(0)
                                                    as isize;
                                                let new_pos = (current_pos + 1)
                                                    .rem_euclid(siblings as isize)
                                                    as usize;
                                                *last = focusable[new_pos];
                                            }
                                        }
                                    }
                                }
                                KeyCode::Enter => {
                                    if let Some(comp) =
                                        app.settings_root.get_child(app.settings_focus_stack[0])
                                    {
                                        if comp.get_children_count() > 0 {
                                            let focusable = comp.get_focusable_indexes();
                                            let next = if focusable.is_empty() {
                                                0
                                            } else {
                                                focusable[0]
                                            };
                                            app.settings_focus_stack.push(next);
                                        }
                                    }
                                }
                                KeyCode::Esc => {
                                    if app.settings_focus_stack.len() > 1 {
                                        app.settings_focus_stack.pop();
                                    } else {
                                        // Сначала меняем состояние, затем вызываем обновление
                                        {
                                            let mut st = app.app_state.borrow_mut();
                                            st.screen = Screen::Simulation;
                                        } // здесь `st` освобождается
                                        app.update_simulation_from_settings();
                                    }
                                }
                                _ => {
                                    if let Some(comp) =
                                        app.settings_root.get_child(app.settings_focus_stack[0])
                                    {
                                        comp.handle_event(&ev, Rc::clone(&app.app_state));
                                    }
                                }
                            }
                        }
                    } else {
                        if let Some(comp) = app.settings_root.get_child(app.settings_focus_stack[0])
                        {
                            comp.handle_event(&ev, Rc::clone(&app.app_state));
                        }
                    }
                }
                Screen::Simulation => {
                    if let Event::Key(key) = &ev {
                        if key.kind == KeyEventKind::Press && key.code == KeyCode::Esc {
                            app.app_state.borrow_mut().screen = Screen::Settings;
                        } else {
                            app.handle_simulation_input(&ev);
                        }
                    } else {
                        app.handle_simulation_input(&ev);
                    }
                }
            }
        }
    }

    crossterm::execute!(
        io::stdout(),
        crossterm::event::DisableMouseCapture,
        crossterm::terminal::LeaveAlternateScreen
    )?;
    crossterm::terminal::disable_raw_mode()?;
    ratatui::restore();
    Ok(())
}
