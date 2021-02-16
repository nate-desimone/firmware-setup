use orbclient::{Color, Renderer};
use orbfont::Font;
use std::{
    proto::Protocol,
    ptr,
};
use uefi::{
    Event,
    Tpl,
    guid::Guid,
    status::{Error, Result},
};

use crate::FONT_TTF;
use crate::display::{Display, Output};
use crate::key::{key, Key};
use crate::rng::Rng;

#[cfg(target_arch = "x86_64")]
unsafe fn wait_for_interrupt() {
    asm!(
        "pushf",
        "sti",
        "hlt",
        "popf"
    );
}

fn confirm() -> Result<()> {
    let uefi = std::system_table();

    let mut display = Display::new(match Output::one() {
        Ok(ok) => ok,
        Err(err) => {
            debugln!("failed to get output: {:?}", err);
            return Err(err);
        }
    });

    let (display_w, display_h) = (display.width(), display.height());

    let scale = if display_h > 1440 {
        4
    } else if display_h > 720 {
        2
    } else {
        1
    };

    let font = match Font::from_data(FONT_TTF) {
        Ok(ok) => ok,
        Err(err) => {
            debugln!("failed to parse font: {}", err);
            return Err(Error::NotFound);
        }
    };

    let rng = match Rng::one() {
        Ok(ok) => ok,
        Err(err) => {
            debugln!("failed to get random number generator: {:?}", err);
            return Err(err);
        }
    };

    // Style {
    let background_color = Color::rgb(0x36, 0x32, 0x2F);
    let text_color = Color::rgb(0xCC, 0xCC, 0xCC);
    let font_size = (16 * scale) as f32;
    // } Style

    // Clear any previous keys
    let _ = key(false);

    let mut texts = Vec::new();
    for message in &[
        "Type in the following code to commence firmware flashing.",
        "The random code is a security measure to ensure you have",
        "physical access to your device.",
        "",
    ] {
        texts.push(font.render(message, font_size));
    }

    let mut code_bytes = [0; 4];
    rng.read(&mut code_bytes)?;
    let code = format!(
        "{:02}{:02}{:02}{:02}",
        code_bytes[0] % 100,
        code_bytes[1] % 100,
        code_bytes[2] % 100,
        code_bytes[3] % 100,
    );
    texts.push(font.render(&code, font_size));

    let mut input = String::new();
    loop {
        let x = font_size as i32;
        let mut y = font_size as i32;

        display.set(background_color);

        for text in texts.iter() {
            text.draw(&mut display, x, y, text_color);
            y += font_size as i32;
        }

        let input_text = font.render(&input, font_size);
        input_text.draw(&mut display, x, y, text_color);
        display.rect(
            x + input_text.width() as i32,
            y,
            font_size as u32 / 2,
            font_size as u32,
            text_color
        );

        display.sync();

        // Since this runs in TPL_CALLBACK, we cannot wait for keys and must spin
        let k = loop {
            match key(false) {
                Ok(ok) => break ok,
                Err(err) => match err {
                    Error::NotReady => {
                        unsafe { wait_for_interrupt(); }
                    },
                    _ => {
                        debugln!("failed to read key: {:?}", err);
                        return Err(err);
                    }
                }
            }
        };
        debugln!("{:?}", k);
        match k {
            Key::Backspace => {
                input.pop();
            },
            Key::Character(c) => {
                input.push(c);
            },
            Key::Enter => {
                if input == code {
                    break;
                }
            },
            _ => {},
        }
    }

    // Clear display
    display.set(Color::rgb(0, 0, 0));
    display.sync();

    Ok(())
}

extern "win64" fn callback(_event: Event, _context: usize) {
    //TODO: check if firmware unlocked
    match confirm() {
        Ok(()) => {
            debugln!("confirmed");
        },
        Err(err) => {
            debugln!("failed to confirm: {:?}", err);
            //TODO: lock and reboot
        }
    }
}

const SYSTEM76_SECURITY_EVENT_GROUP: Guid = Guid(0x764247c4, 0xa859, 0x4a6b, [0xb5, 0x00, 0xed, 0x5d, 0x7a, 0x70, 0x7d, 0xd4]);
const EVT_NOTIFY_SIGNAL: u32 = 0x00000200;
const TPL_CALLBACK: Tpl = Tpl(8);

pub fn install() -> Result<()> {
    let handle = std::handle();
    let uefi = std::system_table();

    let mut event = Event(0);
    (uefi.BootServices.CreateEventEx)(
        EVT_NOTIFY_SIGNAL,
        TPL_CALLBACK,
        callback,
        0,
        &SYSTEM76_SECURITY_EVENT_GROUP,
        &mut event
    )?;

    debugln!("end of dxe event: {:X?}", event);

    Ok(())
}
