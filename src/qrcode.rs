use embedded_graphics::prelude::Point;
use m5_go::M5Go;
use qrcode_generator::{to_image, QrCodeEcc};

pub trait QrCodeDrawer {
    fn draw_qrcode(&mut self, text: &str, size: usize, coeff: usize);
}

impl QrCodeDrawer for M5Go<'_> {
    fn draw_qrcode(&mut self, text: &str, size: usize, coeff: usize) {
        let qr = to_image(text, QrCodeEcc::High, size).expect("Failed to generate QR code");
        let mut qr_slice: Vec<u8> = vec![];
        let mut i = 0;
        let mut line = 0;

        while i < qr.len() {
            for _ in 0..coeff {
                for l in 0..size {
                    for _ in 0..coeff {
                        qr_slice.push(qr[i + l]);
                    }
                }
            }
            self.screen.draw_image(
                qr_slice.as_slice(),
                (size * coeff / 2) as u32,
                Point::new(0, (coeff * line) as i32),
            );
            qr_slice.clear();
            i += size * 2;
            line += 1;
        }
    }
}
