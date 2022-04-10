use std::error::Error;
use std::str::FromStr;

pub fn format_seconds(value: f64) -> String {
    fn div_rem(value: f64, div: usize) -> (usize, f64) {
        ((value / (div as f64)) as usize, value % (div as f64))
    }

    let sign = if value.is_sign_negative() { '-' } else { '+' };

    let value = value.abs();
    let (d, value) = div_rem(value, 86400);
    let (h, value) = div_rem(value, 3600);
    let (m, s) = div_rem(value, 60);

    match (d, h, m, s) {
        (0, 0, m, s) => format!("{}{}:{:06.03}", sign, m, s),
        (0, h, m, s) => format!("{}{}:{:02}:{:06.03}", sign, h, m, s),
        (d, h, m, s) => format!("{}{}:{:02}:{:02}:{:06.03}", sign, d, h, m, s),
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
