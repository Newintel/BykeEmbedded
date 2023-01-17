use nmea_parser::chrono::{DateTime, Utc};
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
    pub coords: Option<Coordinates>,
    pub closest_step: Option<Coordinates>,
    pub time: Option<DateTime<Utc>>,
}

impl InfoState {
    pub fn new() -> Self {
        Self {
            coords: None,
            closest_step: None,
            time: None,
        }
    }
}

pub struct OptionsState {
    pub selected: usize,
    pub max_selected: usize,
    pub fill_on_click: bool,
}

pub struct ConnectionState {
    pub ble: BleState,
    pub request_sent: bool,
}

pub struct State {
    pub main: MainState,
    pub qr: QrState,
    pub current_screen: ScreenId,
    pub infos: InfoState,
    pub options: OptionsState,
    pub connection: ConnectionState,
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
            infos: InfoState::new(),
            options: OptionsState {
                selected: 0,
                max_selected: 1,
                fill_on_click: false,
            },
            connection: ConnectionState {
                ble: BleState::NONE,
                request_sent: false,
            },
        }
    }
}
