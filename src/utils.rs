pub trait DivRem {
    fn div_rem(&self, div: Self) -> (usize, Self);
}

impl DivRem for f64 {
    fn div_rem(&self, div: f64) -> (usize, f64) {
        ((*self / div) as usize, *self % div)
    }
}

pub fn format_seconds(value: f64) -> String {
    let sign = if value.is_sign_negative() { '-' } else { '+' };

    let value = value.abs();
    let (d, value) = value.div_rem(86400.0);
    let (h, value) = value.div_rem(3600.0);
    let (m, s) = value.div_rem(60.0);

    match (d, h, m, s) {
        (0, 0, m, s) => format!("{}{}:{:06.03}", sign, m, s),
        (0, h, m, s) => format!("{}{}:{:02}:{:06.03}", sign, h, m, s),
        (d, h, m, s) => format!("{}{}:{:02}:{:02}:{:06.03}", sign, d, h, m, s),
    }
}
