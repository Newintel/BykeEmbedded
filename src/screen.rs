use std::{
    cell::RefCell,
    sync::{Arc, Mutex},
};

use critical_section::CriticalSection;
use embedded_graphics::{
    mono_font::MonoTextStyle,
    pixelcolor::Rgb565,
    prelude::{Point, RgbColor, Size},
    primitives::{Primitive, PrimitiveStyleBuilder, Rectangle},
    text::{Alignment, Text},
    Drawable,
};

use m5_go::M5GoScreenDriver;
use nmea_parser::{chrono::NaiveTime, gnss::GgaQualityIndicator, ParsedMessage};
use shared::{BleState, Commands, Coordinates, TextSize};

use crate::{gps::read_gps_line, qrcode::draw_qrcode, send_i2c, state::State};

const WIDTH: u32 = 320;
const HEIGHT: u32 = 240;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    A = 1,
    B,
    C,
}

pub struct GraphicBox {
    style_builder: PrimitiveStyleBuilder<Rgb565>,
    drawable: Rectangle,
    color: Rgb565,
    filled: bool,
    must_draw: bool,
    visible: bool,
    text: String,
    text_size: TextSize,
    qr_code: bool,
    id: BoxId,
}

#[derive(PartialEq, Eq)]
pub enum BoxId {
    None,
    ButtonA,
    ButtonB,
    ButtonC,
    Id(usize),
    StrId(String),
}

trait ToId<T> {
    fn to_id(id: T) -> BoxId;
}

impl ToId<usize> for BoxId {
    fn to_id(id: usize) -> BoxId {
        BoxId::Id(id)
    }
}

impl ToId<&str> for BoxId {
    fn to_id(id: &str) -> BoxId {
        BoxId::StrId(String::from(id))
    }
}

macro_rules! id {
    ($id:expr) => {
        BoxId::to_id($id)
    };
}

trait GetBoxId {
    fn get_id(&self, id: BoxId) -> Option<&GraphicBox>;
    fn get_id_mut(&mut self, id: BoxId) -> Option<&mut GraphicBox>;
}

impl GraphicBox {
    pub fn new(position: Point, size: Size) -> Self {
        Self {
            style_builder: PrimitiveStyleBuilder::new(),
            drawable: Rectangle::new(position, size),
            color: Rgb565::BLACK,
            filled: false,
            must_draw: true,
            visible: true,
            text: String::new(),
            text_size: TextSize::Small,
            qr_code: false,
            id: BoxId::None,
        }
    }

    pub fn with_color(mut self, color: Rgb565) -> Self {
        self.color = color;
        self
    }

    pub fn with_text(mut self, text: &str) -> Self {
        self.text = String::from(text);
        self
    }

    pub fn with_text_size(mut self, text_size: TextSize) -> Self {
        self.text_size = text_size;
        self
    }

    pub fn with_qr_code(mut self) -> Self {
        self.qr_code = true;
        self
    }

    pub fn with_id(mut self, id: BoxId) -> Self {
        self.id = id;
        self
    }

    pub fn with_filled(mut self, filled: bool) -> Self {
        self.filled = filled;
        self
    }

    pub fn draw_qr_code(
        &mut self,
        driver: &mut M5GoScreenDriver,
        text: &str,
        size: usize,
        coeff: usize,
    ) {
        draw_qrcode(driver, text, size, coeff, self.drawable.top_left)
    }

    pub fn draw(&mut self, driver: &mut M5GoScreenDriver) {
        let color = if self.filled && self.visible {
            self.color
        } else {
            Rgb565::BLACK
        };

        let border_color = if self.visible {
            self.color
        } else {
            Rgb565::BLACK
        };

        let text_color = if self.visible {
            if self.color == Rgb565::BLACK {
                Rgb565::WHITE
            } else if self.filled {
                Rgb565::BLACK
            } else {
                self.color
            }
        } else {
            Rgb565::BLACK
        };

        let font = self.text_size.get_font();

        let character_style = MonoTextStyle::new(&font, text_color);

        let text_position = Point::new(
            self.drawable.top_left.x + self.drawable.size.width as i32 / 2,
            self.drawable.bottom_right().expect("No bottom right").y
                - self.drawable.size.height as i32 / 2
                + font.baseline as i32 / 2,
        );

        let text_drawable = Text::with_alignment(
            self.text.as_str(),
            text_position,
            character_style,
            Alignment::Center,
        );

        self.drawable
            .into_styled(
                self.style_builder
                    .fill_color(color)
                    .stroke_color(border_color)
                    .stroke_width(1)
                    .build(),
            )
            .draw(driver)
            .ok()
            .or_else(|| {
                println!("Draw rectangle failed");
                None
            });

        if self.visible {
            text_drawable.draw(driver).ok().or_else(|| {
                println!("Draw text failed");
                None
            });
        }
        self.must_draw = false;
    }

    pub fn set_filled(&mut self, filled: bool) {
        self.must_draw = self.filled != filled;
        self.filled = filled;
    }

    pub fn set_visible(&mut self, visible: bool) {
        self.must_draw = self.visible != visible;
        self.visible = visible;
    }

    pub fn set_text(&mut self, text: &str) {
        if self.text == text {
            return;
        }
        self.text = String::from(text);
        self.must_draw = true;
    }

    pub fn replace_text(&mut self, f: impl FnOnce(&str) -> String) {
        let text = f(self.text.as_str());
        if self.text == text {
            return;
        }
        self.text = text;
        self.must_draw = true;
    }
}

pub struct Screen {
    callbacks: Callbacks,
    boxes: Vec<GraphicBox>,
    pub state: Arc<Mutex<RefCell<State>>>,
}

impl GetBoxId for Vec<GraphicBox> {
    fn get_id(&self, id: BoxId) -> Option<&GraphicBox> {
        self.iter().find(|box_| box_.id == id)
    }

    fn get_id_mut(&mut self, id: BoxId) -> Option<&mut GraphicBox> {
        self.iter_mut().find(|box_| box_.id == id)
    }
}

type Callback =
    dyn Fn(CriticalSection, bool, &mut Vec<GraphicBox>, &mut State) + Send + Sync + 'static;
type UpdateCallback = dyn Fn(CriticalSection, Commands, &mut Vec<GraphicBox>, &mut State, Option<(f32, f32)>)
    + Send
    + Sync
    + 'static;

#[derive(Default)]
pub struct Callbacks {
    pub a: Option<Box<Callback>>,
    pub b: Option<Box<Callback>>,
    pub c: Option<Box<Callback>>,
    pub update: Option<Box<UpdateCallback>>,
}

impl Callbacks {
    pub fn get_callback(&self, button: Button) -> Option<&Box<Callback>> {
        match button {
            Button::A => self.a.as_ref(),
            Button::B => self.b.as_ref(),
            Button::C => self.c.as_ref(),
        }
    }

    pub fn get_update_callback(&self) -> Option<&Box<UpdateCallback>> {
        self.update.as_ref()
    }
}

impl Screen {
    fn new_internal(state: Arc<Mutex<RefCell<State>>>) -> Self {
        Self {
            callbacks: Callbacks::default(),
            boxes: vec![],
            state,
        }
    }

    pub fn new(state: Arc<Mutex<RefCell<State>>>) -> Self {
        Self::new_internal(state)
            .add_box(GraphicBox::new(Point::new(0, 0), Size::new(WIDTH, HEIGHT)))
            .add_box(
                GraphicBox::new(Point::new(0, HEIGHT as i32 - 25), Size::new(WIDTH / 3, 25))
                    .with_color(Rgb565::RED)
                    .with_id(BoxId::ButtonA),
            )
            .add_box(
                GraphicBox::new(
                    Point::new(WIDTH as i32 / 3, HEIGHT as i32 - 25),
                    Size::new(WIDTH / 3, 25),
                )
                .with_color(Rgb565::GREEN)
                .with_id(BoxId::ButtonB),
            )
            .add_box(
                GraphicBox::new(
                    Point::new(WIDTH as i32 / 3 * 2, HEIGHT as i32 - 25),
                    Size::new(WIDTH / 3, 25),
                )
                .with_color(Rgb565::BLUE)
                .with_id(BoxId::ButtonC),
            )
    }

    pub fn with_btn_text(mut self, button: Button, text: &str) -> Self {
        let index = button as usize;
        self.boxes[index].text = text.to_string();
        self
    }

    pub fn on<F>(mut self, button: Button, f: F) -> Self
    where
        F: Fn(CriticalSection, bool, &mut Vec<GraphicBox>, &mut State) + Send + Sync + 'static,
    {
        match button {
            Button::A => self.callbacks.a = Some(Box::new(f)),
            Button::B => self.callbacks.b = Some(Box::new(f)),
            Button::C => self.callbacks.c = Some(Box::new(f)),
        }
        self
    }

    pub fn on_update<F>(mut self, f: F) -> Self
    where
        F: Fn(CriticalSection, Commands, &mut Vec<GraphicBox>, &mut State, Option<(f32, f32)>)
            + Send
            + Sync
            + 'static,
    {
        self.callbacks.update = Some(Box::new(f));
        self
    }

    pub fn call(&mut self, cs: CriticalSection, button: Button, pushed: bool) {
        self.state.try_lock().ok().and_then(|mut state| {
            let state = state.get_mut();
            self.boxes
                .get_mut(button as usize)
                .unwrap()
                .set_filled(state.options.fill_on_click && pushed);

            if let Some(f) = self.callbacks.get_callback(button) {
                f(cs, pushed, &mut self.boxes, state);
            }

            Some(())
        });
    }

    pub fn update(
        &mut self,
        cs: CriticalSection,
        command: Option<Commands>,
        c_h: Option<(f32, f32)>,
    ) {
        self.state.try_lock().ok().and_then(|mut state| {
            let state = state.get_mut();
            if let Some(Commands::BleState(s)) = &command {
                state.connection.ble = s.clone();
            }
            if let Some(f) = self.callbacks.get_update_callback() {
                f(cs, command.unwrap_or_default(), &mut self.boxes, state, c_h);
            }
            Some(())
        });
    }

    pub fn add_box(mut self, box_: GraphicBox) -> Self {
        self.boxes.push(box_);
        self
    }

    pub fn display_button(mut self, button: Button, visible: bool) -> Self {
        let index = button as usize;
        self.boxes[index].set_visible(visible);
        self
    }

    pub fn draw(&mut self, driver: &mut M5GoScreenDriver) {
        for box_ in self.boxes.iter_mut() {
            if box_.must_draw {
                box_.draw(driver);
                if box_.qr_code {
                    self.state.try_lock().ok().and_then(|state| {
                        let mut state = state.borrow_mut();
                        let mac = String::from(state.qr.get_mac());
                        if mac.is_empty() == false && state.qr.qr_code_drawn == false {
                            box_.draw_qr_code(driver, mac.as_str(), 200, 2);
                            state.qr.qr_code_drawn = true
                        }
                        Some(())
                    });
                }
            }
        }
    }
}

pub struct App {
    screens: Vec<Screen>,
    pub state: Arc<Mutex<RefCell<State>>>,
    pub on_screen: ScreenId,
}

#[derive(Default, Copy, Clone, PartialEq, Eq)]
pub enum ScreenId {
    #[default]
    Main,
    QrCode,
    Infos,
    Options,
}

impl From<usize> for ScreenId {
    fn from(number: usize) -> Self {
        match number {
            0 => Self::Main,
            1 => Self::QrCode,
            2 => Self::Infos,
            3 => Self::Options,
            _ => Self::default(),
        }
    }
}

impl Into<usize> for ScreenId {
    fn into(self) -> usize {
        match self {
            Self::Main => 0,
            Self::QrCode => 1,
            Self::Infos => 2,
            Self::Options => 3,
        }
    }
}

impl App {
    pub fn new() -> Self {
        let state = Arc::new(Mutex::new(RefCell::new(State::new())));
        Self {
            screens: vec![],
            state,
            on_screen: ScreenId::Main,
        }
    }

    pub fn setup(&mut self) {
        let main_screen = Screen::new(Arc::clone(&self.state))
            .with_btn_text(Button::C, "OK")
            .with_btn_text(Button::B, "Bas")
            .with_btn_text(Button::A, "Haut")
            .on(Button::A, |_, pushed, boxes, state| {
                if state.main.selected > 0 && pushed == false {
                    boxes
                        .get_id_mut(id!(state.main.selected))
                        .and_then(|el| Some(el.replace_text(|txt| txt.replace("> ", ""))));
                    state.main.selected -= 1;
                    boxes
                        .get_id_mut(id!(state.main.selected))
                        .and_then(|el| Some(el.replace_text(|txt| format!("> {}", txt))));
                }
            })
            .on(Button::B, |_, pushed, boxes, state| {
                if state.main.selected < state.main.max_selected && pushed == false {
                    boxes
                        .get_id_mut(id!(state.main.selected))
                        .and_then(|el| Some(el.replace_text(|txt| txt.replace("> ", ""))));
                    state.main.selected += 1;
                    boxes
                        .get_id_mut(id!(state.main.selected))
                        .and_then(|el| Some(el.replace_text(|txt| format!("> {}", txt))));
                }
            })
            .on(Button::C, |_, pushed, boxes, state| {
                if pushed == false {
                    boxes.into_iter().for_each(|box_| box_.must_draw = true);
                    state.current_screen = ScreenId::from(state.main.selected + 1);
                }
            })
            .add_box(
                GraphicBox::new(Point::new(0, 0), Size::new(WIDTH, 25))
                    .with_text("BYKE")
                    .with_text_size(TextSize::Large),
            )
            .add_box(
                GraphicBox::new(Point::new(0, 50), Size::new(WIDTH, 25))
                    .with_text("> Connexion Bluetooth")
                    .with_id(id!(0)),
            )
            .add_box(
                GraphicBox::new(Point::new(0, 75), Size::new(WIDTH, 25))
                    .with_text("Excursion info")
                    .with_id(id!(1)),
            )
            .add_box(
                GraphicBox::new(Point::new(0, 100), Size::new(WIDTH, 25))
                    .with_text("Options")
                    .with_id(id!(2)),
            );

        let qr_code_screen = Screen::new(Arc::clone(&self.state))
            .with_btn_text(Button::C, "Retour")
            .with_btn_text(Button::B, "Redemander QR Code")
            .with_btn_text(Button::A, "Relancer BLE")
            .on_update(|_, command, boxes, state, _| {
                if state.qr.must_get_mac() {
                    critical_section::with(|cs| {
                        send_i2c(cs, Commands::GetMac).and_then(|_| {
                            state.qr.mac_requested();
                            Some(())
                        })
                    });
                }
                match command {
                    Commands::Mac(mac) => {
                        state.qr.set_mac(mac);
                        boxes.get_id_mut(id!("qr")).unwrap().must_draw = true
                    }
                    _ => {}
                };

                boxes
                    .get_id_mut(BoxId::ButtonA)
                    .unwrap()
                    .set_visible(match state.connection.ble {
                        BleState::Disconnected => true,
                        _ => false,
                    });
            })
            .on(Button::C, |_, pushed, boxes, state| {
                if pushed == false {
                    boxes.into_iter().for_each(|box_| box_.must_draw = true);
                    state.qr.qr_code_drawn = false;
                    state.current_screen = ScreenId::Main;
                }
            })
            .on(Button::A, |_, pushed, _, state| {
                if pushed == false && state.connection.ble == BleState::Disconnected {
                    critical_section::with(|cs| send_i2c(cs, Commands::StartBle)).or_else(|| {
                        esp_println::println!("Error sending StartBle command");
                        None
                    });
                }
            })
            .on(Button::B, |cs, pushed, boxes, state| {
                if pushed == false {
                    boxes.get_id_mut(id!("qr")).and_then(|box_| {
                        state.qr.reset();
                        box_.must_draw = true;
                        Some(())
                    });
                    send_i2c(cs, Commands::GetMac)
                        .and_then(|_| {
                            state.qr.mac_requested();
                            Some(())
                        })
                        .or_else(|| {
                            esp_println::println!("Error sending GetMac command");
                            None
                        });
                }
            })
            .add_box(
                GraphicBox::new(Point::new(0, 0), Size::new(200, 200))
                    .with_text("En attente du QR Code")
                    .with_qr_code()
                    .with_id(id!("qr")),
            );

        let infos_screen = Screen::new(Arc::clone(&self.state))
            .with_btn_text(Button::C, "Retour")
            .with_btn_text(Button::B, "Nouvelle etape")
            .with_btn_text(Button::A, "Check connection")
            .on(Button::A, |cs, pushed, _, state| {
                if pushed == false {
                    match state.connection.ble {
                        BleState::Connected | BleState::Advertising | BleState::Disconnected => {
                            send_i2c(cs, Commands::StartBle);
                        }
                        BleState::NONE => {
                            send_i2c(cs, Commands::GetBleState);
                        }
                        _ => {}
                    }
                }
            })
            .on(Button::C, |_, pushed, boxes, state| {
                if pushed == false {
                    boxes.into_iter().for_each(|box_| box_.must_draw = true);
                    state.current_screen = ScreenId::Main;
                }
            })
            .on(Button::B, |cs, pushed, _, state| {
                if pushed == false {
                    state.infos.coords.as_ref().and_then(|coords| {
                        if coords.is_valid() {
                            send_i2c(
                                cs,
                                Commands::NewStep(Coordinates::new(coords.lat, coords.long)),
                            );
                        }
                        Some(())
                    });
                }
            })
            .on_update(|cs, command, boxes, state, c_h| {
                match command {
                    Commands::ClosestStep(coords) => {
                        if coords.is_valid() {
                            state.infos.closest_step = Some(coords);
                        }
                    }
                    Commands::BleState(ble_state) => {
                        let box_a = boxes.get_id_mut(BoxId::ButtonA).unwrap();
                        match ble_state {
                            BleState::Connected
                            | BleState::Advertising
                            | BleState::Disconnected => {
                                box_a.set_visible(true);
                                box_a.set_text("Relancer BLE");
                            }
                            BleState::NONE => {
                                box_a.set_visible(true);
                                box_a.set_text("Check connection");
                            }
                            _ => {}
                        }
                        state.connection.ble = ble_state;
                        state.connection.request_sent = false;
                    }
                    _ => {}
                }
                if state.connection.ble == BleState::NONE && state.connection.request_sent == false
                {
                    send_i2c(cs, Commands::GetBleState);
                    state.connection.request_sent = true;
                } else if state.connection.ble != BleState::Connected {
                    let connection_box = boxes.get_id_mut(id!("connectionState")).unwrap();
                    connection_box.set_visible(true);
                    connection_box.replace_text(|text| {
                        match state.connection.ble {
                            BleState::Disconnected => "L'appareil BLE n'est pas connecte!",
                            BleState::Advertising => "En attente de connexion GPS...",
                            BleState::NONE => "L'appareil BLE est-il allume?",
                            _ => text,
                        }
                        .to_string()
                    });
                } else {
                    boxes
                        .get_id_mut(id!("connectionState"))
                        .unwrap()
                        .set_visible(false);

                    boxes
                        .get_id_mut(BoxId::ButtonA)
                        .unwrap()
                        .set_text("Relancer BLE");

                    boxes
                        .get_id_mut(BoxId::ButtonB)
                        .unwrap()
                        .set_visible(state.infos.coords.is_none());
                }

                if let Some((temperature, humidity)) = c_h {
                    boxes.get_id_mut(id!("temperature")).and_then(|box_| {
                        box_.set_text(format!("Temperature: {:.0}C", temperature).as_str());
                        Some(())
                    });

                    boxes.get_id_mut(id!("humidity")).and_then(|box_| {
                        box_.set_text(format!("Humidite: {:.0}%", humidity).as_str());
                        Some(())
                    });
                }

                match read_gps_line(cs) {
                    Some(message) => {
                        match message {
                            ParsedMessage::Incomplete => {}
                            ParsedMessage::Gga(infos) => {
                                if infos.quality != GgaQualityIndicator::Invalid {
                                    state.infos.time = infos.timestamp;
                                    state.infos.coords = infos.longitude.and_then(|lon| {
                                        infos
                                            .latitude
                                            .and_then(|lat| Some(Coordinates::new(lat, lon)))
                                    });
                                }
                                boxes.get_id_mut(id!("time")).unwrap().replace_text(|text| {
                                    match state.infos.time {
                                        Some(timestamp) => {
                                            let time = timestamp
                                                .time()
                                                .signed_duration_since(NaiveTime::default());
                                            format!(
                                                "{}:{} UTC",
                                                time.num_hours(),
                                                time.num_minutes() - time.num_hours() * 60
                                            )
                                            .to_string()
                                        }
                                        None => text.to_string(),
                                    }
                                });

                                boxes.get_id_mut(id!("longitude")).and_then(|box_| {
                                    box_.replace_text(|text| {
                                        if infos.quality != GgaQualityIndicator::Invalid {
                                            infos.longitude.and_then(|lon| {
                                                Some(format!("Longitude: {:.2}", lon).to_string())
                                            })
                                        } else {
                                            None
                                        }
                                        .unwrap_or(text.to_string())
                                    });
                                    Some(())
                                });

                                boxes.get_id_mut(id!("latitude")).and_then(|box_| {
                                    box_.replace_text(|text| {
                                        if infos.quality != GgaQualityIndicator::Invalid {
                                            infos.latitude.and_then(|lat| {
                                                Some(format!("Latitude: {:.2}", lat).to_string())
                                            })
                                        } else {
                                            None
                                        }
                                        .unwrap_or(text.to_string())
                                    });
                                    Some(())
                                });

                                boxes.get_id_mut(id!("altitude")).and_then(|box_| {
                                    box_.replace_text(|text| {
                                        if infos.quality != GgaQualityIndicator::Invalid {
                                            infos.altitude.and_then(|alt| {
                                                Some(format!("Altitude: {:.1}m", alt).to_string())
                                            })
                                        } else {
                                            None
                                        }
                                        .unwrap_or(text.to_string())
                                    });
                                    Some(())
                                });
                            }
                            ParsedMessage::Rmc(infos) => {
                                boxes.get_id_mut(id!("speed")).and_then(|box_| {
                                    box_.replace_text(|_| {
                                        if let Some(true) = infos.status_active {
                                            infos.sog_knots.and_then(|sog| {
                                                let speed = sog * 0.5144 * 3.6;
                                                Some(format!("Vitesse au sol: {:.2}km/h", speed))
                                            })
                                        } else {
                                            None
                                        }
                                        .unwrap_or("Connexion".to_string())
                                    });
                                    Some(())
                                });
                            }
                            _ => {}
                        };
                    }
                    None => {
                        boxes
                            .get_id_mut(id!("time"))
                            .unwrap()
                            .set_text("Connexion...");
                    }
                };
            })
            .add_box(
                GraphicBox::new(Point::new(0, 0), Size::new(WIDTH / 2, 40))
                    .with_text("Connexion...")
                    .with_text_size(TextSize::Medium)
                    .with_id(id!("time")),
            )
            .add_box(
                GraphicBox::new(Point::new(WIDTH as i32 / 2, 0), Size::new(WIDTH / 2, 40))
                    .with_text("Connexion...")
                    .with_text_size(TextSize::Medium)
                    .with_id(id!("temperature")),
            )
            .add_box(
                GraphicBox::new(Point::new(0, 40), Size::new(WIDTH / 2, 40))
                    .with_text("Connexion...")
                    .with_id(id!("longitude")),
            )
            .add_box(
                GraphicBox::new(Point::new(WIDTH as i32 / 2, 40), Size::new(WIDTH / 2, 40))
                    .with_text("Connexion...")
                    .with_id(id!("latitude")),
            )
            .add_box(
                GraphicBox::new(Point::new(0, 80), Size::new(WIDTH / 2, 40))
                    .with_text("Connexion...")
                    .with_id(id!("altitude")),
            )
            .add_box(
                GraphicBox::new(Point::new(WIDTH as i32 / 2, 80), Size::new(WIDTH / 2, 40))
                    .with_text("Connexion...")
                    .with_id(id!("speed")),
            )
            .add_box(
                GraphicBox::new(Point::new(0, 120), Size::new(WIDTH, 40))
                    .with_text("Connexion...")
                    .with_id(id!("humidity")),
            )
            .add_box(
                GraphicBox::new(Point::new(0, 160), Size::new(WIDTH, 40))
                    .with_id(id!("connectionState"))
                    .with_color(Rgb565::RED),
            );

        let options_screen = Screen::new(Arc::clone(&self.state))
            .with_btn_text(Button::A, "Haut")
            .with_btn_text(Button::B, "Bas")
            .on_update(|_, _, boxes, state, _| {
                match state.options.selected {
                    0 => {
                        boxes.get_id_mut(BoxId::ButtonC).unwrap().set_text("OK");
                        boxes.get_id_mut(id!("info")).unwrap().set_visible(false);
                    }
                    1 => {
                        boxes.get_id_mut(BoxId::ButtonC).unwrap().replace_text(|_| {
                            if state.options.fill_on_click {
                                "Desactiver"
                            } else {
                                "Activer"
                            }
                            .to_string()
                        });
                        boxes.get_id_mut(id!("fill")).unwrap().replace_text(|_| {
                            if state.options.fill_on_click {
                                "Actif"
                            } else {
                                "Inactif"
                            }
                            .to_string()
                        });

                        let info_box = boxes.get_id_mut(id!("info")).unwrap();
                        info_box.set_visible(true);
                        info_box.replace_text(|_| {
                            "Remplissage des boutons en bas de l'ecran".to_string()
                        });
                    }
                    _ => {}
                };
            })
            .on(Button::A, |_, pushed, boxes, state| {
                if state.options.selected > 0 && pushed == false {
                    boxes
                        .get_id_mut(id!(state.options.selected))
                        .and_then(|el| Some(el.replace_text(|txt| txt.replace("> ", ""))));
                    state.options.selected -= 1;
                    boxes
                        .get_id_mut(id!(state.options.selected))
                        .and_then(|el| Some(el.replace_text(|txt| format!("> {}", txt))));
                }
            })
            .on(Button::B, |_, pushed, boxes, state| {
                if state.options.selected < state.options.max_selected && pushed == false {
                    boxes
                        .get_id_mut(id!(state.options.selected))
                        .and_then(|el| Some(el.replace_text(|txt| txt.replace("> ", ""))));
                    state.options.selected += 1;
                    boxes
                        .get_id_mut(id!(state.options.selected))
                        .and_then(|el| Some(el.replace_text(|txt| format!("> {}", txt))));
                }
            })
            .on(Button::C, |_, pushed, boxes, state| {
                if pushed == false {
                    match state.options.selected {
                        0 => {
                            boxes.into_iter().for_each(|box_| box_.must_draw = true);
                            state.current_screen = ScreenId::Main;
                        }
                        1 => {
                            state.options.fill_on_click = state.options.fill_on_click == false;
                        }
                        _ => {}
                    }
                }
            })
            .add_box(
                GraphicBox::new(Point::new(0, 50), Size::new(WIDTH / 2, 25))
                    .with_text("> Retour")
                    .with_id(id!(0)),
            )
            .add_box(
                GraphicBox::new(Point::new(0, 80), Size::new(WIDTH / 2, 25))
                    .with_text("Remplissage des boutons")
                    .with_id(id!(1)),
            )
            .add_box(
                GraphicBox::new(Point::new(WIDTH as i32 / 2, 80), Size::new(WIDTH / 2, 25))
                    .with_id(id!("fill"))
                    .with_text("Inactif"),
            )
            .add_box(
                GraphicBox::new(Point::new(0, HEIGHT as i32 - 60), Size::new(WIDTH, 25))
                    .with_id(id!("info")),
            )
            .add_box(
                GraphicBox::new(Point::new(0, 0), Size::new(WIDTH, 25))
                    .with_text("Options")
                    .with_text_size(TextSize::Large),
            );

        self.screens.push(main_screen);
        self.screens.push(qr_code_screen);
        self.screens.push(infos_screen);
        self.screens.push(options_screen);
    }

    pub fn get_screen(&mut self) -> &mut Screen {
        let current_screen = self.state.lock().unwrap().borrow().current_screen;
        self.screens
            .get_mut(Into::<usize>::into(current_screen))
            .unwrap()
    }
}
