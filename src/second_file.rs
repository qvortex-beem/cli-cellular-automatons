use std::io;
use std::time::Duration;

use crossterm::{
    event::{
        DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableFocusChange, EnableMouseCapture, Event, poll, read,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode},
};

pub fn read_event() -> io::Result<()> {
    let mut stdout = io::stdout();
    enable_raw_mode()?;
    execute!(
        stdout,
        EnableBracketedPaste,
        EnableFocusChange,
        EnableMouseCapture
    )?;

    loop {
        if poll(Duration::from_millis(100))? {
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
