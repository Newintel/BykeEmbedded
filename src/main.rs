mod gps;
mod qrcode;
mod screen;

use std::cell::RefCell;

// TODO: Implement an easier borrow for Mutex<RefCell<Option<T>>>
use critical_section::Mutex;
use esp_idf_hal::{
    delay::FreeRtos,
    gpio::{Gpio37, Gpio38, Gpio39, Input, InterruptType, PinDriver},
    i2c::I2cDriver,
    prelude::Peripherals,
};
use esp_idf_sys as _;
use heapless::Vec;
use m5_go::M5Go;
use qrcode::draw_qrcode;
use screen::{MainState, ScreenId, Screens};
use shared::{Commands, Coordinates};

use crate::screen::Button;

static BUTTON_A: Mutex<RefCell<Option<PinDriver<'_, Gpio39, Input>>>> =
    Mutex::new(RefCell::new(None));

static BUTTON_B: Mutex<RefCell<Option<PinDriver<'_, Gpio38, Input>>>> =
    Mutex::new(RefCell::new(None));

static BUTTON_C: Mutex<RefCell<Option<PinDriver<'_, Gpio37, Input>>>> =
    Mutex::new(RefCell::new(None));

static CTS: Mutex<RefCell<Vec<Commands, 20>>> = Mutex::new(RefCell::new(Vec::new()));

static APP: Mutex<RefCell<Option<Screens>>> = Mutex::new(RefCell::new(None));

const STICK: u8 = 0x16;

fn main() -> anyhow::Result<()> {
    // It is necessary to call this function once. Otherwise some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_sys::link_patches();

    let peripherals = Peripherals::take().unwrap();

    let mut m5 = M5Go::new(peripherals)?;

    m5.button_a.set_interrupt_type(InterruptType::AnyEdge)?;
    m5.button_b.set_interrupt_type(InterruptType::AnyEdge)?;
    m5.button_c.set_interrupt_type(InterruptType::AnyEdge)?;

    unsafe {
        m5.button_a.subscribe(on_push_a)?;
        m5.button_b.subscribe(on_push_b)?;
        m5.button_c.subscribe(on_push_c)?;
    }

    let mut screens = Screens::new();
    screens.setup();

    critical_section::with(|cs| {
        *BUTTON_A.borrow(cs).borrow_mut() = Some(m5.button_a);
        *BUTTON_B.borrow(cs).borrow_mut() = Some(m5.button_b);
        *BUTTON_C.borrow(cs).borrow_mut() = Some(m5.button_c);

        APP.replace(cs, Some(screens));
    });

    m5.screen.turn_on();

    let mut qr_code_drawn = false;

    loop {
        let mut buffer = [0u8; 256];
        if m5.port_a.read(STICK, &mut buffer, 100).is_ok() {
            let command = Commands::parse(&buffer).unwrap_or_default();
            println!("received command: {:?}", command);
        }
        critical_section::with(|cs| {
            APP.borrow(cs).borrow_mut().as_mut().and_then(|app| {
                let (screen, id) = app.get_screen();
                if true {
                    screen.draw(&mut m5.screen.driver);

                    if id == ScreenId::QrCode {
                        if qr_code_drawn == false {
                            draw_qrcode(&mut m5.screen.driver, m5.mac.as_str(), 200, 2);
                            qr_code_drawn = true;
                        }
                    } else {
                        qr_code_drawn = false;
                    }
                }
                Some(())
            });

            CTS.borrow_ref_mut(cs).pop().and_then(|command| {
                println!("sending command: {:?}", command);
                m5.port_a
                    .write(STICK, command.get_stream().as_slice(), 100)
                    .ok()
                    .or_else(|| {
                        esp_println::println!("Failed to send command");
                        Some(())
                    })
            })
        });
        FreeRtos::delay_ms(100);
    }
}

fn on_push_a() {
    critical_section::with(|cs| {
        BUTTON_A.borrow(cs).borrow().as_ref().and_then(|btn| {
            APP.borrow(cs).borrow_mut().as_mut().and_then(|app| {
                app.get_screen().0.call(Button::A, btn.is_low());
                Some(())
            });
            if btn.is_low() {
                CTS.borrow_ref_mut(cs)
                    .push(Commands::GetMac)
                    .ok()
                    .or_else(|| {
                        esp_println::println!("CTS is full");
                        Some(())
                    });
            }
            Some(())
        });
    });
}

fn on_push_b() {
    critical_section::with(|cs| {
        BUTTON_B.borrow(cs).borrow().as_ref().and_then(|btn| {
            APP.borrow(cs).borrow_mut().as_mut().and_then(|app| {
                app.get_screen().0.call(Button::B, btn.is_low());
                Some(())
            })
        });
    });
}

fn on_push_c() {
    critical_section::with(|cs| {
        BUTTON_C.borrow(cs).borrow().as_ref().and_then(|btn| {
            APP.borrow(cs).borrow_mut().as_mut().and_then(|app| {
                app.get_screen().0.call(Button::C, btn.is_low());
                Some(())
            })
        });
    });
}

fn goto_screen(id: ScreenId) {
    critical_section::with(|cs| {
        APP.borrow(cs).borrow_mut().as_mut().and_then(|app| {
            app.goto_screen(id).expect("go to failed");
            Some(())
        })
    });
}
