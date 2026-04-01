mod world;
use crate::world::World;

use std::collections::HashMap;
use std::io::{self, Stdout, Write};
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};
use crossterm::{
    cursor::{self, MoveTo},
    event::{
        DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableFocusChange, EnableMouseCapture, Event, MouseButton, MouseEventKind, poll, read,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode},
};

pub fn control_simulation(world: &mut World) -> io::Result<()> {
    let mut stdout = io::stdout();
    enable_raw_mode()?;
    execute!(
        stdout,
        EnableBracketedPaste,
        EnableFocusChange,
        EnableMouseCapture
    )?;
    loop {
        if poll(Duration::from_millis(1000))? {
            match read()? {
                Event::Key(key) => {
                    if key.kind == KeyEventKind::Press {
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => break,
                            KeyCode::Char('t') | KeyCode::Char('T') => {
                                world.tick();
                                world.draw(&mut stdout);
                            }
                            KeyCode::Char('d') => world.debug(),
                            _ => (),
                        }
                    }
                }
                Event::Mouse(event) => {
                    let x = event.column as i64;
                    let y = event.row as i64;
                    match event.kind {
                        MouseEventKind::Down(button) => match button {
                            MouseButton::Left => {
                                world.animate_cell(x, y);
                                world.draw(&mut stdout);
                                // move_n_print(&mut stdout, event.column, event.row, '#');
                            }
                            MouseButton::Right => {
                                world.kill_cell(x, y);
                                world.draw(&mut stdout);
                                // move_n_print(&mut stdout, event.column, event.row, ' ');
                            }
                            _ => (),
                        },
                        MouseEventKind::Drag(button) => match button {
                            MouseButton::Left => match event.modifiers {
                                KeyModifiers::CONTROL => {}
                                _ => {
                                    world.animate_cell(x, y);
                                    world.draw(&mut stdout);
                                }
                            },
                            MouseButton::Right => {
                                world.kill_cell(x, y);
                                world.draw(&mut stdout);
                            }
                            _ => (),
                        },
                        _ => (),
                    }
                }
                _ => (),
            }
        }
    }

    execute!(
        stdout,
        DisableBracketedPaste,
        DisableFocusChange,
        DisableMouseCapture
    )?;
    disable_raw_mode()?;
    Ok(())
}

pub fn move_n_print(out: &mut Stdout, x: u16, y: u16, char: char) {
    execute!(out, MoveTo(x, y));
    write!(out, "{}", char);
    out.flush();
}

pub fn testing_read_event() -> io::Result<()> {
    let mut stdout = io::stdout();
    enable_raw_mode()?;
    execute!(
        stdout,
        EnableBracketedPaste,
        EnableFocusChange,
        EnableMouseCapture
    )?;

    loop {
        if poll(Duration::from_millis(2000))? {
            match read()? {
                Event::Key(key) if key.code == KeyCode::Esc => break,
                Event::FocusGained => println!("FocusGained"),
                Event::FocusLost => println!("FocusLost"),
                Event::Key(event) => match event.kind {
                    KeyEventKind::Press => println!("Key: {:?}", event),
                    _ => (),
                },
                Event::Mouse(event) => println!("Mouse: {:?}", event),
                #[cfg(feature = "bracketed-paste")]
                Event::Paste(data) => println!("Pasted: {:?}", data),
                Event::Resize(width, height) => println!("Size: {}x{}", width, height),
                _ => (),
            }
        }
    }

    execute!(
        stdout,
        DisableBracketedPaste,
        DisableFocusChange,
        DisableMouseCapture
    )?;
    disable_raw_mode()?;
    Ok(())
}

fn main() {
    let mut world = World::new();
    control_simulation(&mut world);
    // testing_read_event();
}
