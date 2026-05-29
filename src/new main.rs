use crossterm::event::{Event, KeyCode, KeyEventKind, read};
use ratatui::{
    DefaultTerminal, Frame, TerminalOptions,
    layout::{self, Alignment, Constraint, Direction, Layout, Rect},
    macros::ratatui_core::widgets,
    style::{Color, Style, Stylize},
    symbols::block,
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, List, ListDirection, ListItem, ListState, Paragraph, Wrap,
    },
};
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell, collections::HashMap, fmt::format, fs, io::{self, Stdout}, path::{Path, PathBuf}, rc::Rc, time::Duration, vec
};

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
    fn can_be_in_focus(&self) -> Ability_to_focus;
    fn get_focusable_indexes(&mut self) -> Vec<usize>;
}

enum Feedback {
    Esc,
    None,
}

#[derive(PartialEq)]
enum Ability_to_focus {
    Can_be_in_focus,
    Cant_be_in_focus,
}

enum ScreenState {
    Settings,
    Simulation,
}

struct AppState {
    screen: ScreenState,
    simulations_settings: HashMap<String, usize>, // настройка симуляции -> ее значение
    error_message: String,
    key_bindings: HashMap<String, String>, // действие -> каноническое имя
}
impl AppState {
    fn new() -> Self {
        let mut x = Self {
            screen: ScreenState::Settings,
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
        if !&last_saves.exists() {
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
        // Создаём все необходимые директории
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

        // Обновляем last_saves.csv
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
            let record: SettingsRecord = result.map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
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
        writer.write_record(&[settings_path.to_str().unwrap(), simulation_path.to_str().unwrap()])?;
        writer.flush()?;
        Ok(())
    }
}

struct App {
    focus_stack: Vec<usize>,
    root: Box<dyn Component>,
    app_state: Rc<RefCell<AppState>>,
}
impl App {
    fn handle_event(&mut self, ev: &Event) {
        if let Event::Key(key) = ev {
            if key.kind == KeyEventKind::Press {
                let app_state_rc = Rc::clone(&self.app_state);
                let focus_stack = self.focus_stack.clone();
                let entered_component = self.get_entered_component(&focus_stack);
                if let Some(component) = entered_component {
                    match component.handle_event(ev, app_state_rc.clone()) {
                        Feedback::Esc => {
                            if self.focus_stack.len() > 2 {
                                self.esc();
                            }
                        }
                        Feedback::None => {}
                    }
                }
                match key.code {
                    KeyCode::Left | KeyCode::Up => {
                        if self.get_focus_siblings() > 1 {
                            let siblings_len = self.get_focus_siblings();
                            self.move_focus(-1, siblings_len);
                        }
                    }
                    KeyCode::Down | KeyCode::Right => {
                        if self.get_focus_siblings() > 1 {
                            let siblings_len = self.get_focus_siblings();
                            self.move_focus(1, siblings_len);
                        }
                    }
                    KeyCode::Enter => {
                        let focus_stack = self.focus_stack.clone();
                        let focus_component = self.get_focus_component(&focus_stack);
                        if let Some(component) = focus_component {
                            if component.get_children_count() > 0 {
                                self.enter();
                                let focus_stack = self.focus_stack.clone();
                                let entered_component = self.get_entered_component(&focus_stack);
                                if let Some(component) = entered_component {
                                    match component.handle_event(ev, app_state_rc.clone()) {
                                        Feedback::Esc => {
                                            if self.focus_stack.len() > 2 {
                                                self.esc();
                                            }
                                        }
                                        Feedback::None => {}
                                    }
                                }
                            }
                        }
                    }
                    KeyCode::Esc => {
                        if self.focus_stack.len() > 2 {
                            self.esc();
                        }
                    }
                    // KeyCode::Char('f') => {
                    //     let mut st = app_state_rc.borrow_mut();
                    //     st.load_last_saves();
                    // }
                    _ => (),
                }
            }
        }
    }
    fn get_focus_component(&mut self, focus_stack: &[usize]) -> Option<&mut Box<dyn Component>> {
        let mut current: &mut Box<dyn Component> = &mut self.root;
        for &i in &focus_stack[1..] {
            current = current.get_child(i)?;
        }
        Some(current)
    }
    fn get_entered_component(&mut self, focus_stack: &[usize]) -> Option<&mut Box<dyn Component>> {
        let focus_stack = self.focus_stack.clone();
        let parent_slice: &[usize] = &focus_stack[..focus_stack.len().saturating_sub(1)];
        if let Some(parent) = self.get_focus_component(parent_slice) {
            Some(parent)
        } else {
            None
        }
    }
    fn current_index_mut(&mut self) -> Option<&mut usize> {
        self.focus_stack.last_mut()
    }
    fn move_focus(&mut self, delta: isize, siblings_len: usize) {
        if siblings_len == 0 {
            return;
        }
        let parent_slice_owned =
            self.focus_stack[..self.focus_stack.len().saturating_sub(1)].to_vec();

        if let Some(parent) = self.get_focus_component(&parent_slice_owned) {
            let focusable = parent.get_focusable_indexes();

            if let Some(last) = self.focus_stack.last_mut() {
                let current_pos =
                    focusable.iter().position(|&ri| ri == *last).unwrap_or(0) as isize;
                let new_pos = (current_pos + delta).rem_euclid(siblings_len as isize) as usize;

                let new_real_index = focusable[new_pos];
                *last = new_real_index;
            }
        }
    }
    fn enter(&mut self) {
        let focus_stack = self.focus_stack.clone();
        let focus = self.get_focus_component(&focus_stack);
        if let Some(component) = focus {
            let focusable = component.get_focusable_indexes();
            if !focusable.is_empty() {
                self.focus_stack.push(focusable[0]);
            } else {
                self.focus_stack.push(0);
            }
        }
    }
    fn esc(&mut self) {
        if self.focus_stack.len() > 1 {
            self.focus_stack.pop();
        }
    }
    fn get_focus_siblings(&mut self) -> usize {
        let focus_stack = self.focus_stack.clone();
        let parent_slice: &[usize] = &focus_stack[..focus_stack.len().saturating_sub(1)];
        if let Some(parent) = self.get_focus_component(parent_slice) {
            parent.get_children_count()
        } else {
            1
        }
    }
    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Fill(1), Constraint::Length(3)])
            .split(area);
        let tips_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Fill(1), Constraint::Fill(1)])
            .split(layout[1]);
        // "esc - назад |  ↑←→↓ - перемещение | enter - подтвердить"
        let tip = Paragraph::new("esc - назад |  ↑←→↓ - перемещение | enter - подтвердить")
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });
        let err_msg_text = {
            let st = Rc::clone(&self.app_state);
            st.borrow_mut().error_message.clone()
        };
        let error_message = Paragraph::new(err_msg_text)
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::Yellow))
            .wrap(Wrap { trim: true });
        self.root.render(
            frame,
            layout[0],
            self.focus_stack.clone(),
            0,
            true,
            Rc::clone(&self.app_state),
        );
        frame.render_widget(tip, tips_layout[0]);
        frame.render_widget(error_message, tips_layout[1]);
    }
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
            title: "".to_string(),
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
    fn handle_event(&mut self, ev: &Event, app_state: Rc<RefCell<AppState>>) -> Feedback {
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
        let borders = {
            if self.borders {
                Borders::ALL
            } else {
                Borders::NONE
            }
        };
        let title = if is_focused {
            self.title.clone()
        } else {
            "".to_string()
        };
        let focus_style = {
            if is_focused && focus_stack.len() == 1 {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::Black)
            }
        };
        let focus_stack_to_children = {
            if !focus_stack.is_empty() {
                focus_stack.clone()[1..].to_vec()
            } else {
                Vec::new()
            }
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
        let app_state_rc = Rc::clone(&app_state);
        for i in 0..self.children.len() {
            self.children[i].render(
                frame,
                layout[i],
                focus_stack_to_children.clone(),
                i,
                is_focused,
                app_state_rc.clone(),
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
            .filter(|(_, c)| c.can_be_in_focus() != Ability_to_focus::Cant_be_in_focus)
            .map(|(i, _)| i)
            .collect()
    }
    fn get_children_count(&mut self) -> usize {
        self.children
            .iter()
            .filter(|component| component.can_be_in_focus() != Ability_to_focus::Cant_be_in_focus)
            .count()
    }
    fn can_be_in_focus(&self) -> Ability_to_focus {
        if self.can_be_in_focus {
            Ability_to_focus::Can_be_in_focus
        } else {
            Ability_to_focus::Cant_be_in_focus
        }
    }
}

struct Label {
    text: String,
    border: bool,
}
impl Component for Label {
    fn handle_event(&mut self, ev: &Event, app_state: Rc<RefCell<AppState>>) -> Feedback {
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
        let borders = {
            if self.border {
                Borders::ALL
            } else {
                Borders::NONE
            }
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
    fn get_child(&mut self, index: usize) -> Option<&mut Box<dyn Component>> {
        None
    }
    fn get_children_count(&mut self) -> usize {
        0
    }
    fn can_be_in_focus(&self) -> Ability_to_focus {
        Ability_to_focus::Cant_be_in_focus
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
        let mut st = app_state.borrow_mut();
        let value = {
            if let Some(value) = st.simulations_settings.get(&id) {
                value.to_string()
            } else {
                "0".to_string()
            }
        };
        let mut x = Self {
            id: id,
            title: "".to_string(),
            value: value.clone(),
            cursor: value.len(),
            editing: false,
            border: true,
        };
        x
    }
    fn get_value(&self) -> i32 {
        self.value.trim().parse::<i32>().unwrap_or(0)
    }
    fn insert_char(&mut self, char: char, app_state: Rc<RefCell<AppState>>) {
        self.value.insert(self.cursor, char);
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
    fn get_child(&mut self, index: usize) -> Option<&mut Box<dyn Component>> {
        None
    }
    fn get_children_count(&mut self) -> usize {
        1
    }
    fn can_be_in_focus(&self) -> Ability_to_focus {
        Ability_to_focus::Can_be_in_focus
    }
    fn handle_event(&mut self, ev: &Event, app_state: Rc<RefCell<AppState>>) -> Feedback {
        self.editing = true;
        if let Event::Key(key) = ev {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Char(ch) => {
                        if ch.is_ascii_digit() {
                            self.insert_char(ch, app_state);
                        } else {
                            app_state.borrow_mut().error_message =
                                "разрешены только цифры".to_string();
                        }
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
                    _ => (),
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
        app_state: Rc<RefCell<AppState>>,
    ) {
        let is_focused =
            !focus_stack.is_empty() && focus_stack[0] == focus_index && has_focused_parent;
        let is_entered = self.editing;
        let borders = {
            if self.border {
                Borders::ALL
            } else {
                Borders::NONE
            }
        };
        let title = self.title.clone();
        let borders_style = {
            if is_focused && focus_stack.len() == 1 || is_entered {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::White)
            }
        };
        let container = Block::default()
            .borders(borders)
            .border_style(borders_style)
            .title(title);
        frame.render_widget(&container, area);
        let inner_area = container.inner(area);
        let input_spans = {
            let input = self.value.clone();
            let pos = self.cursor;
            let chars: Vec<char> = input.chars().collect();
            let mut spans = Vec::new();

            // часть до курсора
            if pos > 0 {
                let before: String = chars[..pos].iter().collect();
                spans.push(Span::raw(before));
            }

            // символ под курсором
            if pos < chars.len() {
                let c = chars[pos];
                spans.push(Span::styled(c.to_string(), Style::default().reversed()));
            } else {
                spans.push(Span::styled(" ", Style::default().reversed()));
            }

            // часть после курсора
            if pos + 1 < chars.len() {
                let after: String = chars[pos + 1..].iter().collect();
                spans.push(Span::raw(after));
            }

            spans
        };
        let text = {
            if is_entered {
                Line::from(input_spans)
            } else {
                Line::from(self.value.clone())
            }
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
    fn get_child(&mut self, index: usize) -> Option<&mut Box<dyn Component>> {
        None
    }
    fn get_children_count(&mut self) -> usize {
        1
    }
    fn can_be_in_focus(&self) -> Ability_to_focus {
        Ability_to_focus::Can_be_in_focus
    }
    fn get_focusable_indexes(&mut self) -> Vec<usize> {
        vec![]
    }
    fn handle_event(&mut self, ev: &Event, app_state: Rc<RefCell<AppState>>) -> Feedback {
        if let Event::Key(key) = ev {
            if key.kind == KeyEventKind::Press && key.code == KeyCode::Enter {
                if let Some(call_back) = &self.callback {
                    call_back(app_state);
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
        app_state: Rc<RefCell<AppState>>,
    ) {
        let is_focused =
            !focus_stack.is_empty() && focus_stack[0] == focus_index && has_focused_parent;
        let color = {
            if is_focused {
                Color::Yellow
            } else {
                Color::White
            }
        };
        let container = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(color));
        frame.render_widget(&container, area);
        let inner_area = container.inner(area);
        let inner = Block::default().style(Style::default().bg(Color::Black));
        frame.render_widget(&inner, inner_area);
    }
}

struct KeybindInput {
    action_id: String, // "pause"
    label: String,     // "пауза"
    value: String,     // "Space"
    editing: bool,
}
impl KeybindInput {
    fn new(action_id: String, label: String, app_state: &Rc<RefCell<AppState>>) -> Self {
        let x = Self {
            action_id,
            label,
            value: "".to_string(),
            editing: false,
        };
        x
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
    fn can_be_in_focus(&self) -> Ability_to_focus {
        Ability_to_focus::Can_be_in_focus
    }
    fn get_child(&mut self, index: usize) -> Option<&mut Box<dyn Component>> {
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
        let borders = {
            if is_focused || is_editing {
                Borders::ALL
            } else {
                Borders::NONE
            }
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
                    let canonical = canonical_key_name(key.code, &app_state);
                    self.set_binding(canonical.clone(), &app_state);
                    self.editing = false;
                    app_state.borrow_mut().error_message.clear();
                    return Feedback::Esc;
                } else {
                    if key.code == KeyCode::Enter {
                        self.editing = true;
                    }
                }
            }
        }
        Feedback::None
    }
}
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    ratatui::run(app)?;
    Ok(())
}

fn app(terminal: &mut DefaultTerminal) -> std::io::Result<()> {
    let mut app_state = Rc::new(RefCell::new(AppState::new()));
    let mut app = App {
        focus_stack: vec![0, 0],
        root: {
            let mut root_container = Container::new();
            root_container.children = vec![
                {
                    let column1 = Container {
                        title: "мир".to_string(),
                        direction: Direction::Vertical,
                        can_be_in_focus: true,
                        borders: true,
                        children: vec![
                            {
                                let cell_count = Container {
                                    title: "".to_string(),
                                    direction: Direction::Vertical,
                                    can_be_in_focus: true,
                                    borders: true,
                                    children: vec![
                                        {
                                            let label = Label {
                                                text: "Количество клеток:".to_string(),
                                                border: true,
                                            };
                                            Box::new(label)
                                        },
                                        {
                                            let mut input = Input::new(
                                                "cells_to_alive".to_string(),
                                                app_state.clone(),
                                            );
                                            input.title = "для выживания:".to_string();
                                            Box::new(input)
                                        },
                                        {
                                            let mut input = Input::new(
                                                "cells_to_birth".to_string(),
                                                app_state.clone(),
                                            );
                                            input.title = "для рождения:".to_string();
                                            Box::new(input)
                                        },
                                    ],
                                };
                                Box::new(cell_count)
                            },
                            {
                                let sim_speed = Container {
                                    title: "".to_string(),
                                    direction: Direction::Vertical,
                                    can_be_in_focus: true,
                                    borders: true,
                                    children: vec![
                                        {
                                            let label = Label {
                                                text: "Скорость симуляции:".to_string(),
                                                border: true,
                                            };
                                            Box::new(label)
                                        },
                                        {
                                            let mut input = Input::new(
                                                "simulation_speed".to_string(),
                                                app_state.clone(),
                                            );
                                            Box::new(input)
                                        },
                                    ],
                                };
                                Box::new(sim_speed)
                            },
                            {
                                let buttons = Container {
                                    title: "".to_string(),
                                    direction: Direction::Horizontal,
                                    can_be_in_focus: true,
                                    borders: true,
                                    children: vec![
                                        {
                                            let column1 = Container {
                                                title: "".to_string(),
                                                direction: Direction::Vertical,
                                                can_be_in_focus: false,
                                                borders: false,
                                                children: vec![
                                                    {
                                                        let label = Label {
                                                            text: "действие".to_string(),
                                                            border: true,
                                                        };
                                                        Box::new(label)
                                                    },
                                                    {
                                                        let label = Label {
                                                            text: "сохранить".to_string(),
                                                            border: true,
                                                        };
                                                        Box::new(label)
                                                    },
                                                    {
                                                        let label = Label {
                                                            text: "импорт".to_string(),
                                                            border: true,
                                                        };
                                                        Box::new(label)
                                                    },
                                                    {
                                                        let label = Label {
                                                            text: "экспорт".to_string(),
                                                            border: true,
                                                        };
                                                        Box::new(label)
                                                    },
                                                ],
                                            };
                                            Box::new(column1)
                                        },
                                        {
                                            let column2 = Container {
                                                title: "".to_string(),
                                                direction: Direction::Vertical,
                                                can_be_in_focus: true,
                                                borders: true,
                                                children: vec![
                                                    {
                                                        let label = Label {
                                                            text: "настройки".to_string(),
                                                            border: true,
                                                        };
                                                        Box::new(label)
                                                    },
                                                    {
                                                        let button = Button::new(|state| {
                                                            if let Err(e) = state.borrow().save_current_settings() {
                                                                state.borrow_mut().error_message = format!("Ошибка сохранения: {}", e);
                                                            } else {
                                                                state.borrow_mut().error_message = "Настройки сохранены".to_string();
                                                            }
                                                        });
                                                        Box::new(button)
                                                    },
                                                    {
                                                        let button = Button::new(|state| {
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
                                                        });
                                                        Box::new(button)
                                                    },
                                                    {
                                                        let button = Button::new(|state| {
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
                                                        });
                                                        Box::new(button)
                                                    },
                                                ],
                                            };
                                            Box::new(column2)
                                        },
                                        {
                                            let column3 = Container {
                                                title: "".to_string(),
                                                direction: Direction::Vertical,
                                                can_be_in_focus: true,
                                                borders: true,
                                                children: vec![
                                                    {
                                                        let label = Label {
                                                            text: "симуляция".to_string(),
                                                            border: true,
                                                        };
                                                        Box::new(label)
                                                    },
                                                    {
                                                        let button = Button::new(|state| {
                                                            
                                                        });
                                                        Box::new(button)
                                                    },
                                                    {
                                                        let button = Button::new(|state| {
                                                            
                                                        });
                                                        Box::new(button)
                                                    },
                                                    {
                                                        let button = Button::new(|state| {
                                                            
                                                        });
                                                        Box::new(button)
                                                    },
                                                ],
                                            };
                                            Box::new(column3)
                                        },
                                    ],
                                };
                                Box::new(buttons)
                            },
                        ],
                    };
                    Box::new(column1)
                },
                {
                    let column2 = Container {
                        title: "".to_string(),
                        direction: Direction::Vertical,
                        can_be_in_focus: true,
                        borders: true,
                        children: vec![
                            Box::new(KeybindInput::new("pause".to_string(), "пауза".to_string(), &app_state)),
                            Box::new(KeybindInput::new("right".to_string(), "→".to_string(), &app_state)),
                            Box::new(KeybindInput::new("left".to_string(), "←".to_string(), &app_state)),
                            Box::new(KeybindInput::new("up".to_string(), "↑".to_string(), &app_state)),
                            Box::new(KeybindInput::new("down".to_string(), "↓".to_string(), &app_state)),
                            Box::new(KeybindInput::new("zoom_in".to_string(), "приблизить".to_string(), &app_state)),
                            Box::new(KeybindInput::new("zoom_out".to_string(), "отдалить".to_string(), &app_state)),
                            Box::new(KeybindInput::new("speed_up".to_string(), "ускорить".to_string(), &app_state)),
                            Box::new(KeybindInput::new("speed_down".to_string(), "замедлить".to_string(), &app_state)),
                            Box::new(KeybindInput::new("step".to_string(), "совершить одну итерацию".to_string(), &app_state)),
                            Box::new(KeybindInput::new("to_settings".to_string(), "выход в настройки".to_string(), &app_state)),
                        ],
                    };
                    Box::new(column2)
                },
                {
                    let column3 = Container {
                        title: "".to_string(),
                        direction: Direction::Vertical,
                        can_be_in_focus: true,
                        borders: true,
                        children: vec![],
                    };
                    Box::new(column3)
                },
            ];
            Box::new(root_container)
        },
        app_state: app_state,
    };

    loop {
        terminal.draw(|frame| {
            render(frame, &mut app);
        })?;
        let event = read()?;
        match event {
            Event::Key(key) => {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q')
                        | KeyCode::Char('Q')
                        | KeyCode::Char('й')
                        | KeyCode::Char('Й') => break,
                        _ => app.handle_event(&event),
                    }
                }
            }
            _ => (),
        }
    }
    Ok(())
}
fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    app.render(frame, area);
}
fn canonical_key_name(key: KeyCode, app_state: &Rc<RefCell<AppState>>) -> String {
    match key {
        KeyCode::Char(c) => {
            let latin_upper = match c {
                'а'..='я' | 'А'..='Я' | 'ё' | 'Ё' => match c.to_ascii_lowercase() {
                    'й' => "Q", 'ц' => "W", 'у' => "E", 'к' => "R", 'е' => "T",
                    'н' => "Y", 'г' => "U", 'ш' => "I", 'щ' => "O", 'з' => "P",
                    'х' => "{", 'ъ' => "}", 'ф' => "A", 'ы' => "S", 'в' => "D",
                    'а' => "F", 'п' => "G", 'р' => "H", 'о' => "J", 'л' => "K",
                    'д' => "L", 'ж' => ":", 'э' => "\'", 'я' => "Z", 'ч' => "X",
                    'с' => "C", 'м' => "V", 'и' => "B", 'т' => "N", 'ь' => "M",
                    'б' => "<", 'ю' => ">", '.' => "/", '\\' => "\\", ' ' => "Space",
                    '_' => "-", '=' => "+",
                    _ => &c.to_ascii_uppercase().to_string(),
                },
                '.' => "/",
                '\\' => "\\",
                ' ' => "Space",
                '_' => "-",
                '=' => "+",
                _ => &c.to_uppercase().to_string(),
            };
            latin_upper.to_string()
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
        _ => {
            app_state.borrow_mut().error_message = "Недопустимое значение".to_string();
            "?".to_string()
        }
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
    map.insert("to_settings".to_string(), "Esc".to_string());
    map
}
fn default_simulation_settings() -> HashMap<String, usize> {
    let mut map = HashMap::new();
    map.insert("cells_to_alive".to_string(), 1);
    map.insert("cells_to_birth".to_string(), 3);
    map.insert("simulation_speed".to_string(), 1);
    map
}