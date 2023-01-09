use std::{
    cell::RefCell,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use critical_section::CriticalSection;
use embedded_graphics::{
    mono_font::MonoTextStyle,
    pixelcolor::Rgb565,
    prelude::{DrawTarget, Point, RgbColor, Size},
    primitives::{Primitive, PrimitiveStyleBuilder, Rectangle},
    text::{Alignment, Text},
    Drawable,
};

use shared::{BleState, Commands, TextSize};

use crate::{qrcode::draw_qrcode, send_i2c, state::State, GPS};

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

    pub fn draw_qr_code<D>(&mut self, driver: &mut D, text: &str, size: usize, coeff: usize)
    where
        D: DrawTarget<Color = Rgb565>,
        <D as DrawTarget>::Error: std::fmt::Debug,
    {
        draw_qrcode(driver, text, size, coeff, self.drawable.top_left)
    }

    pub fn draw<D>(&mut self, driver: &mut D)
    where
        D: DrawTarget<Color = Rgb565>,
        <D as DrawTarget>::Error: std::fmt::Debug,
    {
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
        self.filled = filled;
        self.must_draw = true;
    }

    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
        self.must_draw = true;
    }

    pub fn set_text(&mut self, text: &str) {
        self.text = String::from(text);
        self.must_draw = true;
    }

    pub fn replace_text(&mut self, f: impl FnOnce(&str) -> String) {
        self.text = f(self.text.as_str());
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
type UpdateCallback =
    dyn Fn(CriticalSection, Commands, &mut Vec<GraphicBox>, &mut State) + Send + Sync + 'static;

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
        F: Fn(CriticalSection, Commands, &mut Vec<GraphicBox>, &mut State) + Send + Sync + 'static,
    {
        self.callbacks.update = Some(Box::new(f));
        self
    }

    pub fn call(&mut self, cs: CriticalSection, button: Button, pushed: bool) {
        self.boxes
            .get_mut(button as usize)
            .unwrap()
            .set_filled(pushed);
        if let Some(f) = self.callbacks.get_callback(button) {
            self.state
                .try_lock()
                .and_then(|mut state| Ok(f(cs, pushed, &mut self.boxes, state.get_mut())))
                .ok();
        }
    }

    pub fn update(&mut self, cs: CriticalSection, command: Commands) {
        self.state.try_lock().ok().and_then(|mut state| {
            let state = state.get_mut();
            if let Commands::BleState(s) = &command {
                state.ble = s.clone();
            }
            if let Some(f) = self.callbacks.get_update_callback() {
                f(cs, command, &mut self.boxes, state);
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

    pub fn draw<D>(&mut self, driver: &mut D)
    where
        D: DrawTarget<Color = Rgb565>,
        <D as DrawTarget>::Error: std::fmt::Debug,
    {
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

pub struct Screens {
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

impl Screens {
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
                        .get_id_mut(BoxId::Id(state.main.selected))
                        .and_then(|el| Some(el.replace_text(|txt| txt.replace("> ", ""))));
                    state.main.selected -= 1;
                    boxes
                        .get_id_mut(BoxId::Id(state.main.selected))
                        .and_then(|el| Some(el.replace_text(|txt| format!("> {}", txt))));
                }
            })
            .on(Button::B, |_, pushed, boxes, state| {
                if state.main.selected < state.main.max_selected && pushed == false {
                    boxes
                        .get_id_mut(BoxId::Id(state.main.selected))
                        .and_then(|el| Some(el.replace_text(|txt| txt.replace("> ", ""))));
                    state.main.selected += 1;
                    boxes
                        .get_id_mut(BoxId::Id(state.main.selected))
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
                    .with_id(BoxId::Id(0)),
            )
            .add_box(
                GraphicBox::new(Point::new(0, 75), Size::new(WIDTH, 25))
                    .with_text("Excursion info")
                    .with_id(BoxId::Id(1)),
            )
            .add_box(
                GraphicBox::new(Point::new(0, 100), Size::new(WIDTH, 25))
                    .with_text("Options")
                    .with_id(BoxId::Id(2)),
            );

        let qr_code_screen = Screen::new(Arc::clone(&self.state))
            .with_btn_text(Button::A, "Relancer BLE")
            .with_btn_text(Button::B, "Redemander QR Code")
            .with_btn_text(Button::C, "Retour")
            .on_update(|_, command, boxes, state| {
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
                        boxes
                            .get_id_mut(BoxId::StrId(String::from("qr")))
                            .unwrap()
                            .must_draw = true
                    }
                    _ => {}
                };

                boxes.get_mut(1).unwrap().set_visible(match state.ble {
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
            .on(Button::A, |_, pushed, _, _| {
                if pushed == false {
                    critical_section::with(|cs| send_i2c(cs, Commands::StartBle)).or_else(|| {
                        esp_println::println!("Error sending StartBle command");
                        None
                    });
                }
            })
            .on(Button::B, |_, pushed, boxes, state| {
                if pushed == false {
                    boxes
                        .get_id_mut(BoxId::StrId(String::from("qr")))
                        .and_then(|box_| {
                            state.qr.reset();
                            box_.must_draw = true;
                            Some(())
                        });
                    critical_section::with(|cs| {
                        send_i2c(cs, Commands::GetMac)
                            .and_then(|_| {
                                state.qr.mac_requested();
                                Some(())
                            })
                            .or_else(|| {
                                esp_println::println!("Error sending GetMac command");
                                None
                            })
                    });
                }
            })
            .add_box(
                GraphicBox::new(Point::new(0, 0), Size::new(200, 200))
                    .with_text("En attente du QR Code")
                    .with_qr_code()
                    .with_id(BoxId::StrId(String::from("qr"))),
            );

        let infos_screen = Screen::new(Arc::clone(&self.state))
            .with_btn_text(Button::C, "Retour")
            .with_btn_text(Button::B, "Nouvelle etape")
            .on(Button::C, |_, pushed, boxes, state| {
                if pushed == false {
                    boxes.into_iter().for_each(|box_| box_.must_draw = true);
                    state.current_screen = ScreenId::Main;
                }
            })
            .on_update(|cs, _, _, _| {
                GPS.borrow_ref_mut(cs).as_mut().and_then(|gps| {
                    match gps.read_line() {
                        crate::gps::ConnectionState::Connected(_) => {}
                        crate::gps::ConnectionState::NoConnection => {}
                    };
                    Some(())
                });
            })
            .add_box(
                GraphicBox::new(Point::new(0, 0), Size::new(WIDTH, 25))
                    .with_text("Excursion info")
                    .with_text_size(TextSize::Large),
            )
            .add_box(GraphicBox::new(Point::new(0, 50), Size::new(WIDTH, 25)));

        let options_screen = Screen::new(Arc::clone(&self.state))
            .with_btn_text(Button::C, "Retour")
            .on(Button::C, |_, pushed, boxes, state| {
                if pushed == false {
                    boxes.into_iter().for_each(|box_| box_.must_draw = true);
                    state.current_screen = ScreenId::Main;
                }
            })
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
