use embedded_graphics::{
    image::{Image, ImageRawBE},
    pixelcolor::Rgb565,
    prelude::{DrawTarget, Point},
    Drawable,
};
use qrcode_generator::{to_image, QrCodeEcc};

pub fn draw_qrcode<D>(driver: &mut D, text: &str, size: usize, coeff: usize, position: Point)
where
    D: DrawTarget<Color = Rgb565>,
    <D as DrawTarget>::Error: std::fmt::Debug,
{
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
        let image_raw = ImageRawBE::<Rgb565>::new(&qr_slice, (size * coeff / 2) as u32);
        let image = Image::new(&image_raw, position + Point::new(0, (coeff * line) as i32));
        image.draw(driver).expect("Failed drawing image");
        qr_slice.clear();
        i += size * 2;
        line += 1;
    }
}
