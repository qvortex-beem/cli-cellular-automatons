mod world;
use crate::camera::Camera;
use crate::world::World;
mod camera;

use std::collections::HashMap;
use std::io::{self, Stdout, Write};
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{self, size};
use crossterm::{
    cursor::{self, MoveTo},
    event::{
        DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableFocusChange, EnableMouseCapture, Event, MouseButton, MouseEventKind, poll, read,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode},
};

pub fn control_simulation(world: &mut World, camera: &mut Camera) -> io::Result<()> {
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
                            KeyCode::Char('q')
                            | KeyCode::Char('Q')
                            | KeyCode::Char('й')
                            | KeyCode::Char('Й')
                            | KeyCode::Esc => break,
                            KeyCode::Char('t')
                            | KeyCode::Char('T')
                            | KeyCode::Char('е')
                            | KeyCode::Char('Е') => {
                                world.tick();
                                world.draw(&mut stdout, *camera);
                            }
                            KeyCode::Left => {
                                if camera.pos_x > 0 {
                                    camera.pos_x -= 1;
                                    world.draw(&mut stdout, *camera);
                                }
                            }
                            KeyCode::Up => {
                                if camera.pos_y > 0 {
                                    camera.pos_y -= 1;
                                    world.draw(&mut stdout, *camera);
                                }
                            }
                            KeyCode::Right => {
                                camera.pos_x += 1;
                                world.draw(&mut stdout, *camera);
                            }
                            KeyCode::Down => {
                                camera.pos_y += 1;
                                world.draw(&mut stdout, *camera);
                            }
                            KeyCode::Char('=') | KeyCode::Char('+') => {
                                camera.zoom += 1;
                            }
                            _ => (),
                        }
                    }
                }
                Event::Mouse(event) => {
                    let (x, y) = camera.screen_to_world(event.column, event.row);
                    match event.kind {
                        MouseEventKind::Down(button) => match button {
                            MouseButton::Left => {
                                world.animate_cell(x, y);
                                world.lazy_draw(&mut stdout, *camera, x, y, true);
                            }
                            MouseButton::Right => {
                                world.kill_cell(x, y);
                                world.lazy_draw(&mut stdout, *camera, x, y, false);
                            }
                            _ => (),
                        },
                        MouseEventKind::Drag(button) => match button {
                            MouseButton::Left => match event.modifiers {
                                KeyModifiers::CONTROL => {}
                                _ => {
                                    world.animate_cell(x, y);
                                    world.lazy_draw(&mut stdout, *camera, x, y, true);
                                }
                            },
                            MouseButton::Right => {
                                world.kill_cell(x, y);
                                world.lazy_draw(&mut stdout, *camera, x, y, false);
                            }
                            _ => (),
                        },
                        _ => (),
                    }
                }
                Event::Resize(x, y) => {
                    camera.term_width = x;
                    camera.term_height = y;
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
    let (term_width, term_height) = size().unwrap();
    let mut cam = Camera::new(term_width, term_height);
    control_simulation(&mut world, &mut cam);
    // testing_read_event();
}
