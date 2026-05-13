use crossterm::event;
use crossterm::event::{Event, KeyCode, KeyEventKind, read};
use ratatui::layout::Alignment;
use ratatui::widgets::Wrap;
use ratatui::{
    DefaultTerminal, Frame, TerminalOptions,
    layout::{self, Constraint, Direction, Layout, Rect},
    style::{Color, Style, Stylize},
    symbols::block,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::error::Error;
use std::io::{self, Stdout};
use std::path;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;
use std::{fs, vec};

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
    fn handle_event(&mut self, ev: &Event, app_state: Rc<RefCell<AppState>>);
    fn get_child(&mut self, index: usize) -> Option<&mut Box<dyn Component>>;
    fn get_children_count(&mut self) -> usize;
    fn get_component_type(&self) -> Component_type;
    fn get_focusable_indexes(&mut self) -> Vec<usize>;
}

#[derive(PartialEq)]
enum Component_type {
    Container,
    Label,
    Input,
    Button,
}

enum ScreenState {
    Settings,
    Simulation,
}

struct AppState {
    screen: ScreenState,
    simulations_settings: HashMap<String, usize>,
    error_message: String,
}
impl AppState {
    fn new() -> Self {
        let mut x = Self {
            screen: ScreenState::Settings,
            simulations_settings: HashMap::new(),
            error_message: String::new(),
        };
        x.simulations_settings
            .insert("cells_to_alive".to_string(), 1);
        x.simulations_settings
            .insert("cells_to_birth".to_string(), 3);
        x.simulations_settings
            .insert("simulation_speed".to_string(), 1);
        x
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
                    component.handle_event(ev, app_state_rc.clone());
                }
                match key.code {
                    KeyCode::Left | KeyCode::Up => {
                        let siblings_len = self.get_focus_siblings();
                        self.move_focus(-1, siblings_len);
                    }
                    KeyCode::Down | KeyCode::Right => {
                        let siblings_len = self.get_focus_siblings();
                        self.move_focus(1, siblings_len);
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
                                    component.handle_event(ev, app_state_rc.clone());
                                }
                            }
                        }
                    }
                    KeyCode::Esc => {
                        if self.focus_stack.len() > 2 {
                            self.esc();
                        }
                    }
                    KeyCode::Char('f') => {
                        let mut st = app_state_rc.borrow_mut();
                        st.error_message = self.focus_stack.iter().map(|i| i.to_string()).collect();
                    }
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
    children: Vec<Box<dyn Component>>,
}
impl Container {
    fn new() -> Self {
        Self {
            title: "".to_string(),
            direction: Direction::Horizontal,
            children: vec![],
        }
    }
    fn add_child(&mut self, child: Box<dyn Component>) {
        self.children.push(child);
    }
}
impl Component for Container {
    fn handle_event(&mut self, ev: &Event, app_state: Rc<RefCell<AppState>>) {}
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
        let borders = Borders::ALL;
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
            .filter(|(_, c)| c.get_component_type() != Component_type::Label)
            .map(|(i, _)| i)
            .collect()
    }
    fn get_children_count(&mut self) -> usize {
        self.children
            .iter()
            .filter(|component| component.get_component_type() != Component_type::Label)
            .count()
    }
    fn get_component_type(&self) -> Component_type {
        Component_type::Container
    }
}

struct Label {
    text: String,
}
impl Component for Label {
    fn handle_event(&mut self, ev: &Event, app_state: Rc<RefCell<AppState>>) {}
    fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        focus_stack: Vec<usize>,
        focus_index: usize,
        has_focused_parent: bool,
        app_state: Rc<RefCell<AppState>>,
    ) {
        let borders = Borders::ALL;
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
    fn get_component_type(&self) -> Component_type {
        Component_type::Label
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
    fn get_component_type(&self) -> Component_type {
        Component_type::Input
    }
    fn handle_event(&mut self, ev: &Event, app_state: Rc<RefCell<AppState>>) {
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
        let borders = Borders::ALL;
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
                        children: vec![
                            {
                                let cell_count = Container {
                                    title: "".to_string(),
                                    direction: Direction::Vertical,
                                    children: vec![
                                        {
                                            let label = Label {
                                                text: "Количество клеток:".to_string(),
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
                                    children: vec![
                                        {
                                            let label = Label {
                                                text: "Скорость симуляции:".to_string(),
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
                                    direction: Direction::Vertical,
                                    children: vec![],
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
                        children: vec![],
                    };
                    Box::new(column2)
                },
                {
                    let column3 = Container {
                        title: "".to_string(),
                        direction: Direction::Vertical,
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
