use std::fmt::write;
use std::io::{self, Stdout, Write};
use std::time::Duration;

use crossterm::event::{KeyCode, KeyModifiers};
use crossterm::{
    cursor::{self, MoveTo},
    event::{
        DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableFocusChange, EnableMouseCapture, Event, MouseButton, MouseEventKind, poll, read,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode},
};

pub fn control_simulation() -> io::Result<()> {
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
                Event::Key(key) => match key.code {
                    KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => break,
                    _ => (),
                },
                Event::Mouse(event) => match event.kind {
                    MouseEventKind::Down(button) => match button {
                        MouseButton::Left => {
                            move_n_print(&mut stdout, event.column, event.row, '#');
                        }
                        MouseButton::Right => {
                            move_n_print(&mut stdout, event.column, event.row, ' ');
                        }
                        _ => (),
                    },
                    MouseEventKind::Drag(button) => match button {
                        MouseButton::Left => match event.modifiers {
                            KeyModifiers::CONTROL => {}
                            _ => move_n_print(&mut stdout, event.column, event.row, '#'),
                        },
                        MouseButton::Right => {
                            move_n_print(&mut stdout, event.column, event.row, ' ');
                        }
                        _ => (),
                    },
                    _ => (),
                },
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

fn move_n_print(out: &mut Stdout, x: u16, y: u16, char: char) {
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
                Event::Key(key) if key.code == crossterm::event::KeyCode::Esc => break,
                Event::FocusGained => println!("FocusGained"),
                Event::FocusLost => println!("FocusLost"),
                Event::Key(event) => println!("Key: {:?}", event),
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
