use std::str::from_utf8;

use anyhow::anyhow;
use embedded_graphics::mono_font::{
    ascii::{FONT_10X20, FONT_6X13},
    MonoFont,
};
use profont::PROFONT_24_POINT;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct Coordinates {
    pub lat: f64,
    pub long: f64,
}

#[derive(Debug)]
pub enum Commands {
    NONE,
    NewStep(Coordinates),
    NextStep(Coordinates),
    GetNextStep,
    GetMac,
    Mac(String),
    OK,
    StartBle,
    StopBle,
}

impl Default for Commands {
    fn default() -> Self {
        Commands::NONE
    }
}

impl From<u8> for Commands {
    fn from(code: u8) -> Self {
        match code {
            0x00 => Commands::NONE,
            0x01 => Commands::NewStep(Coordinates::default()),
            0x02 => Commands::NextStep(Coordinates::default()),
            0x03 => Commands::GetNextStep,
            0x04 => Commands::OK,
            0x05 => Commands::GetMac,
            0x06 => Commands::Mac("".to_string()),
            0x07 => Commands::StartBle,
            0x08 => Commands::StopBle,
            _ => Commands::NONE,
        }
    }
}

impl Commands {
    pub fn get_code(&self) -> u8 {
        match self {
            Commands::NONE => 0x00,
            Commands::NewStep(_) => 0x01,
            Commands::NextStep(_) => 0x02,
            Commands::GetNextStep => 0x03,
            Commands::OK => 0x04,
            Commands::GetMac => 0x05,
            Commands::Mac(_) => 0x06,
            Commands::StartBle => 0x07,
            Commands::StopBle => 0x08,
        }
    }

    fn get_info(&self) -> String {
        match self {
            Commands::NewStep(coords) | Commands::NextStep(coords) => {
                serde_json::to_string(&coords).unwrap()
            }
            Commands::OK => "OK".to_string(),
            Commands::Mac(mac) => mac.to_string(),
            _ => "".to_string(),
        }
    }

    pub fn get_stream(&self) -> Vec<u8> {
        let data = format!("{}", self.get_info());
        let mut stream = vec![self.get_code(), data.len() as u8];
        stream.extend_from_slice(data.as_bytes());
        stream
    }

    pub fn parse(stream: &[u8]) -> anyhow::Result<(Self, usize)> {
        if stream.len() < 2 {
            return Err(anyhow!("Invalid command"));
        }
        let code = stream[0];
        let command = Commands::from(code);

        let length = stream[1] as usize;
        let data = if length > 2 && length + 2 <= stream.len() {
            Some(&stream[2..length + 2])
        } else {
            None
        };

        if command.get_code() == Commands::NONE.get_code() {
            return Ok((Commands::NONE, length));
        }

        if code == Commands::GetNextStep.get_code() {
            return Ok((Commands::GetNextStep, length));
        }

        if code == Commands::OK.get_code() {
            return Ok((Commands::OK, length));
        }

        if code == Commands::GetMac.get_code() {
            return Ok((Commands::GetMac, length));
        }

        if code == Commands::StartBle.get_code() {
            return Ok((Commands::StartBle, length));
        }

        if code == Commands::StopBle.get_code() {
            return Ok((Commands::StopBle, length));
        }

        if data.is_none() {
            return Ok((Commands::NONE, length));
        }

        let data = data.unwrap();

        if code == Commands::Mac(Default::default()).get_code() {
            let mac = from_utf8(data).unwrap();
            return Ok((Commands::Mac(mac.to_string()), length));
        }

        serde_json::from_slice::<'_, Coordinates>(data)
            .ok()
            .and_then(|coords| {
                if code == Commands::NewStep(Default::default()).get_code() {
                    Some((Commands::NewStep(coords), length))
                } else if code == Commands::NextStep(Default::default()).get_code() {
                    Some((Commands::NextStep(coords), length))
                } else {
                    None
                }
            })
            .or_else(|| {
                if length > 20 {
                    Some((Commands::NONE, length))
                } else {
                    None
                }
            })
            .ok_or(anyhow!("Invalid command"))
    }
}

pub enum TextSize {
    Small,
    Medium,
    Large,
}

impl TextSize {
    pub fn get_font(&self) -> &'static MonoFont<'static> {
        match self {
            TextSize::Small => &FONT_6X13,
            TextSize::Medium => &FONT_10X20,
            TextSize::Large => &PROFONT_24_POINT,
        }
    }
}
