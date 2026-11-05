//! The weather's state (M43): clear, rain or thunder, counting down to the
//! next change. The game has no rain to draw yet, so this is what
//! `/weather` sets and reports.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Weather {
    #[default]
    Clear,
    Rain,
    Thunder,
}

impl Weather {
    /// For the wire: 0 clear, 1 rain, 2 thunder.
    pub fn to_byte(self) -> u8 {
        self as u8
    }

    pub fn from_byte(byte: u8) -> Self {
        match byte {
            1 => Weather::Rain,
            2 => Weather::Thunder,
            _ => Weather::Clear,
        }
    }

    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "clear" => Some(Weather::Clear),
            "rain" => Some(Weather::Rain),
            "thunder" => Some(Weather::Thunder),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Weather::Clear => "clear",
            Weather::Rain => "rain",
            Weather::Thunder => "thunder",
        }
    }
}

/// Ticks `/weather clear` etc. last without a duration (vanilla: 5-10
/// minutes of rain, 12000-180000 of clear; rain and thunder use 6000).
pub const DEFAULT_TICKS: u32 = 6000;

impl Weather {
    /// What the weather turns into when its time is up, from a roll of
    /// 0–99: clear mostly stays fair, rain sometimes thunders.
    pub fn next(self, roll: u32) -> Self {
        match self {
            Weather::Clear if roll < 35 => Weather::Rain,
            Weather::Clear => Weather::Clear,
            Weather::Rain if roll < 25 => Weather::Thunder,
            Weather::Rain => Weather::Clear,
            Weather::Thunder if roll < 70 => Weather::Rain,
            Weather::Thunder => Weather::Clear,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weather_cycles_between_clear_rain_and_thunder() {
        assert_eq!(Weather::Clear.next(0), Weather::Rain);
        assert_eq!(Weather::Clear.next(99), Weather::Clear);
        assert_eq!(Weather::Rain.next(10), Weather::Thunder);
        assert_eq!(Weather::Rain.next(90), Weather::Clear);
        assert_eq!(Weather::Thunder.next(10), Weather::Rain);
        assert_eq!(Weather::parse("rain"), Some(Weather::Rain));
        assert_eq!(Weather::parse("snow"), None);
    }
}
