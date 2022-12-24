mod gps;
mod qrcode;

use embedded_graphics::{pixelcolor::Rgb565, prelude::RgbColor};
use esp_idf_hal::{delay::FreeRtos, prelude::Peripherals};
use esp_idf_sys as _;
use m5_go::{BleConfig, M5Go};

fn main() -> anyhow::Result<()> {
    // It is necessary to call this function once. Otherwise some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_sys::link_patches();

    let peripherals = Peripherals::take().unwrap();

    let mut m5 = M5Go::new(peripherals)?;

    let config = BleConfig::new()
        .on_receive(|str| Some(format!("Received: {}", String::from_utf8_lossy(str))));

    m5.setup_ble(config);

    let ble = m5.ble.unwrap();

    m5.screen.turn_on();
    m5.screen.fill_background(Rgb565::BLACK);

    loop {
        FreeRtos::delay_ms(1000);
    }
}
