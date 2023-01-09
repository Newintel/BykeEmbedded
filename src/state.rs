use shared::{BleState, Coordinates};

use crate::screen::ScreenId;

pub struct MainState {
    pub selected: usize,
    pub max_selected: usize,
}

pub struct QrState {
    mac: String,
    command_sent: bool,
    pub qr_code_drawn: bool,
}

impl QrState {
    pub fn set_mac(&mut self, mac: String) {
        self.mac = mac;
        self.command_sent = false;
    }

    pub fn mac_requested(&mut self) {
        self.command_sent = true;
    }

    pub fn must_get_mac(&mut self) -> bool {
        self.mac.is_empty() && self.command_sent == false
    }

    pub fn get_mac(&self) -> &String {
        &self.mac
    }

    pub fn reset(&mut self) {
        self.mac = String::new();
        self.command_sent = false;
        self.qr_code_drawn = false;
    }
}

pub struct InfoState {
    coords: Option<Coordinates>,
    next_step: Option<Coordinates>,
}

impl InfoState {
    pub fn new() -> Self {
        Self {
            coords: None,
            next_step: None,
        }
    }

    pub fn set_coords(&mut self, coords: Coordinates) {
        self.coords = Some(coords);
    }

    pub fn set_next_step(&mut self, next_step: Coordinates) {
        self.next_step = Some(next_step);
    }

    pub fn distance_to_next_step(&self) -> Option<f64> {
        if let Some(coords) = &self.coords {
            if let Some(next_step) = &self.next_step {
                return Some(coords.distance(next_step));
            }
        }
        None
    }
}

pub struct State {
    pub main: MainState,
    pub qr: QrState,
    pub current_screen: ScreenId,
    pub ble: BleState,
    pub info: InfoState,
}

impl State {
    pub fn new() -> Self {
        Self {
            main: MainState {
                selected: 0,
                max_selected: 2,
            },
            qr: QrState {
                mac: String::new(),
                command_sent: false,
                qr_code_drawn: false,
            },
            current_screen: ScreenId::Main,
            ble: BleState::NONE,
            info: InfoState::new(),
        }
    }
}
