pub fn si_magnitude<V: Into<f64>>(value: V, unit: &str) -> String {
    let value: f64 = value.into();
    let (magnitude, suffix) = match value.abs() {
        v if v >= 1e12 => (value / 1e12, "T"),
        v if v >= 1e9 => (value / 1e9, "G"),
        v if v >= 1e6 => (value / 1e6, "M"),
        v if v >= 1e3 => (value / 1e3, "k"),
        _ => (value, ""),
    };

    let with_one_decimal_place = (magnitude * 10.0).floor();
    let decimal_places = if magnitude.floor() * 10.0 == with_one_decimal_place {
        0
    } else {
        1
    };

    format!("{magnitude:.*} {suffix}{unit}", decimal_places)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_si_magnitudes() {
        assert_eq!(si_magnitude(500.0, "B"), "500 B");
        assert_eq!(si_magnitude(1500.0, "B"), "1.5 kB");
        assert_eq!(si_magnitude(2_500_000.0, "B"), "2.5 MB");
        assert_eq!(si_magnitude(3_600_000_000.0, "B"), "3.6 GB");
        assert_eq!(si_magnitude(7_200_000_000_000.0, "B"), "7.2 TB");
        assert_eq!(si_magnitude(-1500.0, "B"), "-1.5 kB");
        assert_eq!(si_magnitude(0.0, "B"), "0 B");
    }
}