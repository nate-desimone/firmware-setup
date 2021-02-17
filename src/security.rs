use ectool::{AccessLpcDirect, Ec, SecurityState, Timeout};
use orbclient::{Color, Renderer};
use std::{
    cell::Cell,
    proto::Protocol,
    ptr,
};
use uefi::{
    Event,
    Tpl,
    guid::Guid,
    reset::ResetType,
    status::{Error, Result, Status},
};

use crate::display::{Display, Output};
use crate::key::{key, Key};
use crate::rng::Rng;
use crate::ui::Ui;

pub struct UefiTimeout {
    duration: u64,
    elapsed: Cell<u64>,
}

impl UefiTimeout {
    pub fn new(duration: u64) -> Self {
        Self {
            duration,
            elapsed: Cell::new(0),
        }
    }
}

impl Timeout for UefiTimeout {
    fn reset(&mut self) {
        self.elapsed.set(0);
    }

    fn running(&self) -> bool {
        let elapsed = self.elapsed.get() + 1;
        let _ = (std::system_table().BootServices.Stall)(1);
        self.elapsed.set(elapsed);
        elapsed < self.duration
    }
}

#[cfg(target_arch = "x86_64")]
unsafe fn wait_for_interrupt() {
    asm!(
        "pushf",
        "sti",
        "hlt",
        "popf"
    );
}

fn confirm(display: &mut Display, security_state: SecurityState) -> Result<()> {
    let (_display_w, display_h) = (display.width(), display.height());

    let scale = if display_h > 1440 {
        4
    } else if display_h > 720 {
        2
    } else {
        1
    };

    // Style {
    let margin_tb = 4 * scale;

    let font_size = (16 * scale) as f32;
    // } Style

    let ui = Ui::new()?;

    let rng = match Rng::one() {
        Ok(ok) => ok,
        Err(err) => {
            debugln!("failed to get random number generator: {:?}", err);
            return Err(err);
        }
    };

    // Clear any previous keys
    let _ = key(false);

    let mut texts = Vec::new();

    //TODO: remove debugging
    texts.push(ui.font.render(&format!("Security State: {:?}", security_state), font_size));

    for message in &[
        "Type in the following code to commence firmware flashing.",
        "The random code is a security measure to ensure you have",
        "physical access to your device.",
        "",
    ] {
        texts.push(ui.font.render(message, font_size));
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
    texts.push(ui.font.render(&code, font_size));

    let mut button_i = 0;
    let buttons = [
        ui.font.render("Confirm", font_size),
        ui.font.render("Cancel", font_size),
    ];

    let mut max_input = String::new();
    while max_input.len() < code.len() {
        // 0 is the widest number with Fira Sans
        max_input.push('0');
    }
    let max_input_text = ui.font.render(&max_input, font_size);

    let mut input = String::new();
    loop {
        let x = font_size as i32;
        let mut y = font_size as i32;

        display.set(ui.background_color);

        for text in texts.iter() {
            text.draw(display, x, y, ui.text_color);
            y += font_size as i32;
        }
        y += margin_tb;

        let input_text = ui.font.render(&input, font_size);
        ui.draw_pretty_box(display, x, y, max_input_text.width(), font_size as u32, false);
        input_text.draw(display, x, y, ui.text_color);
        if input.len() < code.len() {
            display.rect(
                x + input_text.width() as i32,
                y,
                font_size as u32 / 2,
                font_size as u32,
                ui.text_color
            );
        }
        y += font_size as i32 + margin_tb;

        // Blank space
        y += font_size as i32;

        for (i, button_text) in buttons.iter().enumerate() {
            ui.draw_text_box(display, x, y, button_text, i == button_i, i == button_i);
            y += font_size as i32 + margin_tb;
        }

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
        debugln!("key: {:?}", k);
        match k {
            Key::Backspace => {
                input.pop();
            },
            Key::Character(c) => {
                match c {
                    '0' | '1' | '2' | '3' | '4' | '5' | '6' | '7' | '8' | '9' => {
                        if input.len() < code.len() {
                            input.push(c);
                        }
                    }
                    _ => (),
                }
            },
            Key::Enter => {
                if button_i == 0 {
                    if input == code {
                        // Continue if code entered
                        return Ok(());
                    } else {
                        // Clear invalid input
                        input.clear();
                    }
                } else {
                    // Return error if cancel selected
                    return Err(Error::Aborted);
                }
            },
            Key::Escape => {
                input.clear();
            },
            Key::Down => {
                if button_i + 1 < buttons.len() {
                    button_i += 1;
                }
            },
            Key::Up => {
                if button_i > 0 {
                    button_i -= 1;
                }
            },
            _ => {},
        }
    }
}

extern "win64" fn callback(_event: Event, _context: usize) {
    let access = match unsafe { AccessLpcDirect::new(UefiTimeout::new(100_000)) } {
        Ok(ok) => ok,
        Err(err) => {
            debugln!("failed to access EC: {:?}", err);
            return;
        },
    };

    let mut ec = match unsafe { Ec::new(access) } {
        Ok(ok) => ok,
        Err(err) => {
            debugln!("failed to probe EC: {:?}", err);
            return;
        },
    };

    let security_state = match unsafe { ec.security_get() } {
        Ok(ok) => ok,
        Err(err) => {
            debugln!("failed to get EC security state: {:?}", err);
            return;
        }
    };

    debugln!("security state: {:?}", security_state);
    match security_state {
        // Already locked, so do not confirm
        SecurityState::Lock => {
            return;
        },
        // Not locked, require confirmation
        _ => (),
    }

    let res = match Output::one() {
        Ok(output) => {
            let mut display = Display::new(output);

            let res = confirm(&mut display, security_state);

            // Clear display
            display.set(Color::rgb(0, 0, 0));
            display.sync();

            res
        },
        Err(err) => {
            debugln!("failed to get output: {:?}", err);
            Err(err)
        }
    };

    match res {
        Ok(()) => {
            debugln!("confirmed");
        },
        Err(err) => {
            debugln!("failed to confirm: {:?}", err);

            // Lock on next shutdown, will power on automatically
            match unsafe { ec.security_set(SecurityState::PrepareLock) } {
                Ok(()) => (),
                Err(err) => {
                    debugln!("failed to prepare to lock EC security state: {:?}", err)
                }
            }

            // Shutdown
            (std::system_table().RuntimeServices.ResetSystem)(
                ResetType::Shutdown,
                Status(0),
                0,
                ptr::null()
            );
        }
    }
}

const SYSTEM76_SECURITY_EVENT_GROUP: Guid = Guid(0x764247c4, 0xa859, 0x4a6b, [0xb5, 0x00, 0xed, 0x5d, 0x7a, 0x70, 0x7d, 0xd4]);
const EVT_NOTIFY_SIGNAL: u32 = 0x00000200;
const TPL_CALLBACK: Tpl = Tpl(8);

pub fn install() -> Result<()> {
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
