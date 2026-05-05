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
    fn render(&self, frame: &mut Frame, area: Rect);
    fn handle_event(&mut self, ev: &Event);
}
enum ScreenState {
    Settings,
    Simulation,
}

struct AppState {
    screen: ScreenState,
    cells_to_alive: usize,
    cells_to_birth: usize,
    simulation_speed: usize,
}
impl AppState {
    fn new() -> Self {
        Self {
            screen: ScreenState::Settings,
            cells_to_alive: 1,
            cells_to_birth: 3,
            simulation_speed: 1,
        }
    }
}

struct App {
    app_state: AppState,
    event_bus: EventBus,
}
impl App {
    fn new() -> Self {
        Self {
            app_state: AppState::new(),
            event_bus: EventBus {
                queue: Vec::new(),
                components: Vec::new(),
            },
        }
    }
}

struct EventBus {
    queue: Vec<Event>,
    components: Vec<Box<dyn Component>>,
}
impl EventBus {
    fn new(components: Vec<Box<dyn Component>>) -> Self {
        Self {
            queue: Vec::new(),
            components: components,
        }
    }
    fn dispatch(&mut self) {
        for event in self.queue.drain(..) {
            for component in self.components.iter_mut() {
                component.handle_event(&event);
            }
        }
    }
    fn add_component(&mut self, component: Box<dyn Component>) {
        self.components.push(component)
    }
}

struct Container {
    direction: Direction,
    components: Vec<Box<dyn Component>>,
}
impl Container {
    fn new(direction: Direction, components: Vec<Box<dyn Component>>) -> Self {
        Self {
            direction: direction,
            components: components,
        }
    }
}
impl Component for Container {
    fn handle_event(&mut self, ev: &Event) {}
    fn render(&self, frame: &mut Frame, area: Rect) {
        let container = Block::default().borders(Borders::ALL);
        frame.render_widget(&container, area);
        if self.components.is_empty() {
            return;
        }
        let inner_area = container.inner(area);
        let layout = Layout::default()
            .direction(self.direction)
            .constraints(std::iter::repeat(Constraint::Fill(1)).take(self.components.len()))
            .split(inner_area);
        for i in 0..self.components.len() {
            self.components[i].render(frame, layout[i]);
        }
    }
}

struct Label {
    title: Option<String>,
    text: String,
}
impl Label {
    fn new(title: Option<String>, text: String) -> Self {
        Self { title, text }
    }
}
impl Component for Label {
    fn handle_event(&mut self, ev: &Event) {}
    fn render(&self, frame: &mut Frame, area: Rect) {
        let text = self.text.clone();
        let title = match self.title.clone() {
            Some(title) => title,
            _ => "".to_string(),
        };
        let label = Paragraph::new(text)
            .block(Block::bordered().title(title))
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: true });
        frame.render_widget(label, area);
    }
}

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    ratatui::run(app)?;
    Ok(())
}

fn app(terminal: &mut DefaultTerminal) -> std::io::Result<()> {
    let mut app = App {
        app_state: AppState::new(),
        event_bus: EventBus {
            queue: Vec::new(),
            components: vec![
                Box::new(Container::new(
                    Direction::Vertical,
                    vec![
                        Box::new(Label::new(None, String::from("text1"))),
                        Box::new(Label::new(Some("title".to_string()), String::from("text2"))),
                        Box::new(Label::new(None, String::from("text3"))),
                        Box::new(Label::new(None, String::from("text4"))),
                    ],
                )),
                Box::new(Container::new(Direction::Vertical, vec![])),
                Box::new(Container::new(
                    Direction::Vertical,
                    vec![
                        Box::new(Container::new(
                            Direction::Horizontal,
                            vec![
                                Box::new(Label::new(None, String::from("text1"))),
                                Box::new(Label::new(None, String::from("text2"))),
                            ],
                        )),
                        Box::new(Container::new(Direction::Vertical, vec![])),
                    ],
                )),
            ],
        },
    };

    loop {
        terminal.draw(|frame| {
            render(frame, &app);
        });
        let event = read()?;
        match event {
            Event::Key(key) => {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q')
                        | KeyCode::Char('Q')
                        | KeyCode::Char('й')
                        | KeyCode::Char('Й')
                        | KeyCode::Esc
                        | KeyCode::Backspace => break,
                        _ => (),
                    }
                }
            }
            _ => (),
        }
    }
    Ok(())
}
fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(vec![
            Constraint::Fill(1),
            Constraint::Fill(1),
            Constraint::Fill(1),
        ])
        .split(area);
    app.event_bus
        .components
        .get(0)
        .unwrap()
        .render(frame, layout[0]);
    app.event_bus
        .components
        .get(1)
        .unwrap()
        .render(frame, layout[1]);
    app.event_bus
        .components
        .get(2)
        .unwrap()
        .render(frame, layout[2]);
}
