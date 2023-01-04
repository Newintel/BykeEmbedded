use std::str::from_utf8;

use anyhow::anyhow;
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
}

impl Default for Commands {
    fn default() -> Self {
        Commands::NONE
    }
}

impl Commands {
    fn get_code(&self) -> u8 {
        match self {
            Commands::NONE => 0x00,
            Commands::NewStep(_) => 0x01,
            Commands::NextStep(_) => 0x02,
            Commands::GetNextStep => 0x03,
            Commands::OK => 0x04,
            Commands::GetMac => 0x05,
            Commands::Mac(_) => 0x06,
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

    pub fn parse(stream: &[u8]) -> anyhow::Result<Self> {
        let code = stream[0];
        let length = stream[1] as usize;
        let data = if length > 0 && length < 254 {
            Some(&stream[2..length + 2])
        } else {
            None
        };

        if code == Commands::NONE.get_code() {
            return Ok(Commands::NONE);
        }

        if code == Commands::GetNextStep.get_code() {
            return Ok(Commands::GetNextStep);
        }

        if code == Commands::OK.get_code() {
            return Ok(Commands::OK);
        }

        if code == Commands::GetMac.get_code() {
            return Ok(Commands::GetMac);
        }

        if data.is_none() {
            return Err(anyhow!("Invalid command"));
        }

        let data = data.unwrap();

        if code == Commands::Mac(Default::default()).get_code() {
            let mac = from_utf8(data).unwrap();
            return Ok(Commands::Mac(mac.to_string()));
        }

        serde_json::from_slice::<'_, Coordinates>(data)
            .ok()
            .and_then(|coords| {
                if code == Commands::NewStep(Default::default()).get_code() {
                    Some(Commands::NewStep(coords))
                } else if code == Commands::NextStep(Default::default()).get_code() {
                    Some(Commands::NextStep(coords))
                } else {
                    None
                }
            })
            .ok_or(anyhow!("Invalid command"))
    }
}
