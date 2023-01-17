use std::time::{Duration, SystemTime};

use critical_section::CriticalSection;
use nmea_parser::{NmeaParser, ParsedMessage};

use crate::UART;

pub fn read_gps_line(cs: CriticalSection) -> Option<ParsedMessage> {
    // A line starts with '$' (code 36), and ends with '\n' (code 10)
    let mut line: Vec<u8> = vec![];
    let start = SystemTime::now();

    UART.borrow_ref(cs).as_ref().and_then(|driver| loop {
        if start.elapsed().unwrap() > Duration::from_secs(1) {
            return None;
        }

        if driver.remaining_read().unwrap() > 0 {
            let mut buf = [0_u8];
            driver.read(&mut buf, 100).unwrap();
            line.extend_from_slice(&buf);

            if line.starts_with("$".as_bytes()) == false {
                line.clear();
            }

            if line.ends_with("\n".as_bytes()) {
                let sentence = String::from_utf8(line).unwrap();
                let mut parser = NmeaParser::new();
                return parser.parse_sentence(sentence.as_str()).ok();
            }
        }
    })
}
