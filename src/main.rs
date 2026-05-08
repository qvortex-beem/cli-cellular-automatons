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
    widgets::{Block, Borders, List, ListDirection, ListItem, ListState, Paragraph},
};
use std::error::Error;
use std::io::{self, Stdout};
use std::path;
use std::path::{Path, PathBuf};
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
    );
    fn handle_event(&mut self, ev: &Event);
    fn get_child(&mut self, index: usize) -> Option<&mut Box<dyn Component>>;
    fn get_children_count(&mut self) -> usize;
    fn get_component_type(&mut self) -> String;
}

struct App {
    tip_text: String,
    tip_focus_text: String,
    focus_stack: Vec<usize>,
    root: Box<dyn Component>,
}
impl App {
    fn handle_event(&mut self, ev: &Event) {
        if let Event::Key(key) = ev {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Left | KeyCode::Up => {
                        let siblings_len = self.get_focus_siblings();
                        self.tip_text = siblings_len.to_string();
                        self.move_focus(-1, siblings_len);
                        self.tip_focus_text =
                            self.focus_stack.iter().map(|int| int.to_string()).collect()
                    }
                    KeyCode::Down | KeyCode::Right => {
                        let siblings_len = self.get_focus_siblings();
                        self.tip_text = siblings_len.to_string();
                        self.move_focus(1, siblings_len);
                        self.tip_focus_text =
                            self.focus_stack.iter().map(|int| int.to_string()).collect()
                    }
                    KeyCode::Enter => {
                        let focus_stack = self.focus_stack.clone();
                        let focus_component = self.get_focus_component(&focus_stack);
                        if let Some(component) = focus_component {
                            if component.get_children_count() > 0 {
                                self.enter();
                            }
                        }
                        self.tip_focus_text =
                            self.focus_stack.iter().map(|int| int.to_string()).collect()
                    }
                    KeyCode::Esc => {
                        if self.focus_stack.len() > 2 {
                            self.esc();
                            self.tip_focus_text =
                                self.focus_stack.iter().map(|int| int.to_string()).collect()
                        }
                    }
                    KeyCode::Char('c') => {
                        let focus_stack = self.focus_stack.clone();
                        let focus_component = self.get_focus_component(&focus_stack);
                        if let Some(component) = focus_component {
                            self.tip_text = component.get_children_count().to_string();
                        }
                    }
                    KeyCode::Char('t') => {
                        let focus_stack = self.focus_stack.clone();
                        let focus_component = self.get_focus_component(&focus_stack);
                        if let Some(component) = focus_component {
                            self.tip_text = component.get_component_type();
                        }
                    }
                    KeyCode::Char(ch) => {
                        let focus_stack = self.focus_stack.clone();
                        let focus_component = self.get_focus_component(&focus_stack);
                        if let Some(component) = focus_component {
                            component.handle_event(ev);
                        }
                    }
                    _ => (),
                }
            }
        }
    }
    fn get_focus_component(&mut self, focus_stack: &[usize]) -> Option<&mut Box<dyn Component>> {
        let mut current: &mut Box<dyn Component> = &mut self.root;
        for i in &focus_stack.clone()[1..] {
            current = current.get_child(*i)?;
        }
        Some(current)
    }
    fn current_index_mut(&mut self) -> Option<&mut usize> {
        self.focus_stack.last_mut()
    }
    fn move_focus(&mut self, delta: isize, siblings_len: usize) {
        if siblings_len == 0 {
            return;
        }
        if let Some(i) = self.current_index_mut() {
            let new_i = (*i as isize + delta).rem_euclid(siblings_len as isize) as usize;
            *i = new_i;
        }
    }
    fn enter(&mut self) {
        self.focus_stack.push(0);
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
        // "esc - назад |  ↑←→↓ - перемещение | enter - подтвердить"
        let tip = Paragraph::new(format!(
            "{} | {}",
            self.tip_text.to_string(),
            self.tip_focus_text.to_string()
        ))
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: true });
        self.root
            .render(frame, layout[0], self.focus_stack.clone(), 0, true);
        frame.render_widget(tip, layout[1]);
    }
}

struct Container {
    children: Vec<Box<dyn Component>>,
}
impl Component for Container {
    fn handle_event(&mut self, ev: &Event) {}
    fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        focus_stack: Vec<usize>,
        focus_index: usize,
        has_focused_parent: bool,
    ) {
        let is_focused =
            !focus_stack.is_empty() && focus_stack[0] == focus_index && has_focused_parent;
        let borders = if is_focused {
            Borders::ALL
        } else {
            Borders::NONE
        };
        let focus_stack_to_children = {
            if !focus_stack.is_empty() {
                focus_stack.clone()[1..].to_vec()
            } else {
                Vec::new()
            }
        };
        let container = Block::default().borders(borders);
        frame.render_widget(&container, area);
        if self.children.is_empty() {
            return;
        }
        let inner_area = container.inner(area);
        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(std::iter::repeat(Constraint::Fill(1)).take(self.children.len()))
            .split(inner_area);
        for i in 0..self.children.len() {
            self.children[i].render(
                frame,
                layout[i],
                focus_stack_to_children.clone(),
                i,
                is_focused,
            );
        }
    }
    fn get_child(&mut self, index: usize) -> Option<&mut Box<dyn Component>> {
        self.children.get_mut(index)
    }
    fn get_children_count(&mut self) -> usize {
        self.children.len()
    }
    fn get_component_type(&mut self) -> String {
        "container".to_string()
    }
}

struct Label {
    text: String,
}
impl Component for Label {
    fn handle_event(&mut self, ev: &Event) {}
    fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        focus_stack: Vec<usize>,
        focus_index: usize,
        has_focused_parent: bool,
    ) {
        let is_focused =
            !focus_stack.is_empty() && focus_stack[0] == focus_index && has_focused_parent;
        let borders = if is_focused {
            Borders::ALL
        } else {
            Borders::NONE
        };
        let container = Block::default().borders(borders);
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
        1
    }
    fn get_component_type(&mut self) -> String {
        "label".to_string()
    }
}

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    ratatui::run(app)?;
    Ok(())
}

fn app(terminal: &mut DefaultTerminal) -> std::io::Result<()> {
    let mut app = App {
        tip_text: "хз".to_string(),
        tip_focus_text: "хз".to_string(),
        focus_stack: vec![0, 0],
        root: Box::new(Container {
            children: vec![
                Box::new(Container {
                    children: vec![
                        Box::new(Label {
                            text: "лейбел1".to_string(),
                        }),
                        Box::new(Label {
                            text: "лейбел".to_string(),
                        }),
                    ],
                }),
                Box::new(Container {
                    children: vec![
                        Box::new(Container {
                            children: vec![
                                Box::new(Label {
                                    text: "лейбел2".to_string(),
                                }),
                                Box::new(Label {
                                    text: "лейбел3".to_string(),
                                }),
                            ],
                        }),
                        Box::new(Container {
                            children: vec![
                                Box::new(Label {
                                    text: "лейбел4".to_string(),
                                }),
                                Box::new(Label {
                                    text: "лейбел5".to_string(),
                                }),
                            ],
                        }),
                    ],
                }),
                Box::new(Container {
                    children: vec![Box::new(Label {
                        text: "лейбел6".to_string(),
                    })],
                }),
            ],
        }),
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
                        | KeyCode::Char('Й')
                        | KeyCode::Backspace => break,
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
