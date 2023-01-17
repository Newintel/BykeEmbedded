mod gps;
mod qrcode;
mod screen;
mod state;

use std::cell::RefCell;

// TODO: Implement an easier borrow for Mutex<RefCell<Option<T>>>
use critical_section::{CriticalSection, Mutex};

use esp_idf_hal::{delay::FreeRtos, gpio::InterruptType, prelude::Peripherals, uart::UartDriver};
use esp_idf_sys as _;
use heapless::Vec;
use m5_go::{leds::Leds, ButtonAType, ButtonBType, ButtonCType, M5Go};
use screen::App;
use shared::Commands;

use crate::screen::Button;

static BUTTON_A: Mutex<RefCell<Option<ButtonAType>>> = Mutex::new(RefCell::new(None));

static BUTTON_B: Mutex<RefCell<Option<ButtonBType>>> = Mutex::new(RefCell::new(None));

static BUTTON_C: Mutex<RefCell<Option<ButtonCType>>> = Mutex::new(RefCell::new(None));

static CTS: Mutex<RefCell<Vec<Commands, 20>>> = Mutex::new(RefCell::new(Vec::new()));

static APP: Mutex<RefCell<Option<App>>> = Mutex::new(RefCell::new(None));

static UART: Mutex<RefCell<Option<UartDriver>>> = Mutex::new(RefCell::new(None));

static LEDS: Mutex<RefCell<Option<Leds>>> = Mutex::new(RefCell::new(None));

const STICK: u8 = 0x16;
const SENSOR: u8 = 0x44;

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

    let mut screens = App::new();
    screens.setup();

    // Activate temperature and humidity sensor
    m5.port_a
        .write(SENSOR, &[0x20, 0x32], 100)
        .ok()
        .or_else(|| {
            println!("Write failed");
            None
        });

    critical_section::with(|cs| {
        BUTTON_A.replace(cs, Some(m5.button_a));
        BUTTON_B.replace(cs, Some(m5.button_b));
        BUTTON_C.replace(cs, Some(m5.button_c));
        UART.replace(cs, Some(m5.port_c));
        LEDS.replace(cs, Some(m5.leds));

        APP.replace(cs, Some(screens));
    });

    m5.screen.turn_on();

    loop {
        let mut buffer = [0u8; 256];
        let command = if m5.port_a.read(STICK, &mut buffer, 50).is_ok() {
            let (command, _) = Commands::parse(&buffer).unwrap_or_default();
            match command {
                Commands::NONE => {}
                _ => println!("received command : {:?}", command),
            };
            Some(command)
        } else {
            None
        };

        let mut sensor_buffer = [0u8; 6];
        let c_h = if m5.port_a.read(SENSOR, &mut sensor_buffer, 50).is_ok() {
            let data = sensor_buffer
                .to_vec()
                .iter_mut()
                .map(|i| f32::from(*i))
                .collect::<Vec<f32, 6>>();

            let c = ((((data[0] * 256.0) + data[1]) * 175.) / 65535.0) - 45.;
            let h = (((data[3] * 256.0) + data[4]) * 100.) / 65535.0;
            Some((c, h))
        } else {
            None
        };

        critical_section::with(|cs| {
            APP.borrow(cs).borrow_mut().as_mut().and_then(|app| {
                let screen = app.get_screen();
                screen.update(cs, command, c_h);
                screen.draw(&mut m5.screen.driver);
                Some(())
            });
            let mut commands = CTS.borrow_ref_mut(cs);
            commands.pop().and_then(|command| {
                println!("sending command: {:?}", command);
                m5.port_a
                    .write(STICK, command.get_stream().as_slice(), 50)
                    .ok()
                    .or_else(|| {
                        println!("Failed to send command");
                        commands.insert(0, command).ok().or_else(|| {
                            println!("The command failed being re-sent");
                            None
                        });
                        Some(())
                    })
            });
        });
        FreeRtos::delay_ms(100);
    }
}

fn on_push_a() {
    critical_section::with(|cs| {
        BUTTON_A.borrow(cs).borrow().as_ref().and_then(|btn| {
            APP.borrow(cs).borrow_mut().as_mut().and_then(|app| {
                app.get_screen().call(cs, Button::A, btn.is_low());
                Some(())
            });
            Some(())
        });
    });
}

fn on_push_b() {
    critical_section::with(|cs| {
        BUTTON_B.borrow(cs).borrow().as_ref().and_then(|btn| {
            APP.borrow(cs).borrow_mut().as_mut().and_then(|app| {
                app.get_screen().call(cs, Button::B, btn.is_low());
                Some(())
            })
        });
    });
}

fn on_push_c() {
    critical_section::with(|cs| {
        BUTTON_C.borrow(cs).borrow().as_ref().and_then(|btn| {
            APP.borrow(cs).borrow_mut().as_mut().and_then(|app| {
                app.get_screen().call(cs, Button::C, btn.is_low());
                Some(())
            })
        });
    });
}

fn send_i2c(cs: CriticalSection, command: Commands) -> Option<()> {
    CTS.borrow_ref_mut(cs).insert(0, command).ok()
}
