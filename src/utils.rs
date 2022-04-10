use std::error::Error;
use std::fmt;
use std::ops::Add;
use std::str::FromStr;

#[derive(Debug, Copy, Clone)]
pub struct Seconds(pub f64);

impl Seconds {
    pub fn new(frame: usize, sample_rate: usize) -> Seconds {
        assert!(sample_rate > 0);
        Seconds(frame as f64 / sample_rate as f64)
    }
}

impl fmt::Display for Seconds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn div_rem(value: f64, div: usize) -> (usize, f64) {
            ((value / (div as f64)) as usize, value % (div as f64))
        }

        let sign = if self.0.is_sign_negative() { '-' } else { '+' };

        let value = self.0.abs();
        let (d, value) = div_rem(value, 86400);
        let (h, value) = div_rem(value, 3600);
        let (m, s) = div_rem(value, 60);

        match (d, h, m, s) {
            (0, 0, m, s) => write!(f, "{}{}:{:06.03}", sign, m, s),
            (0, h, m, s) => write!(f, "{}{}:{:02}:{:06.03}", sign, h, m, s),
            (d, h, m, s) => write!(f, "{}{}:{:02}:{:02}:{:06.03}", sign, d, h, m, s),
        }
    }
}

impl Add<Seconds> for Seconds {
    type Output = Seconds;

    fn add(self, rhs: Seconds) -> Self::Output {
        Seconds(self.0 + rhs.0)
    }
}

pub fn parse_positive_literal<'a, T>(input: &str) -> Result<T, Box<dyn 'a + Error + Send + Sync>>
where
    T: FromStr + Default + PartialOrd,
    <T as FromStr>::Err: 'a + Error + Send + Sync,
{
    let value = input.parse::<T>()?;

    if value > T::default() {
        Ok(value)
    } else {
        Err("not a positive literal".into())
    }
}
