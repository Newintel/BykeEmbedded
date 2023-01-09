use std::time::{Duration, SystemTime};

use esp_idf_hal::uart::UartDriver;
use nmea_parser::{NmeaParser, ParsedMessage};

pub enum ConnectionState {
    Connected(ParsedMessage),
    NoConnection,
}

pub struct GPS<'a> {
    driver: &'a UartDriver<'a>,
    parser: NmeaParser,
}

impl<'a> GPS<'a> {
    pub fn new(driver: &'a UartDriver<'a>) -> Self {
        Self {
            driver,
            parser: NmeaParser::new(),
        }
    }

    pub fn read_line(&mut self) -> ConnectionState {
        // A line starts with '$' (code 36), and ends with '\n' (code 10)
        let mut line: Vec<u8> = vec![];
        let start = SystemTime::now();

        loop {
            if start.elapsed().unwrap() > Duration::from_secs(1) {
                return ConnectionState::NoConnection;
            }

            if self.driver.remaining_read().unwrap() > 0 {
                let mut buf = [0_u8];
                self.driver.read(&mut buf, 100).unwrap();
                line.extend_from_slice(&buf);

                if line.starts_with("$".as_bytes()) == false {
                    line.clear();
                }

                if line.ends_with("\n".as_bytes()) {
                    let sentence = String::from_utf8(line).unwrap();
                    return ConnectionState::Connected(
                        self.parser
                            .parse_sentence(sentence.as_str())
                            .expect(format!("Parsing sentence {} failed", sentence).as_str()),
                    );
                }
            }
        }
    }
}
