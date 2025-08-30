/// Formats a numeric availability percentage into a human-readable string.
/// 
/// Availability figures are on the range [0..100] and we usually think of
/// them in terms of their inverse (i.e. their error rate) being the key
/// indicator of importance. To provide appropriate scaling, we therefore
/// calculate the inverse error rate (100/(100 - availability)) and compute
/// the log_10 of that to determine the number of decimal places of precision
/// to display.
pub fn availability(percentage: f64) -> String {
    if percentage == 100.0 {
        return "100%".into();
    }

    let inverse_error_rate = 100.0 / (100.0 - percentage);
    let decimal_places = (inverse_error_rate.log10().floor() - 1.0).max(0.0) as usize;
    format!("{percentage:.*}%", decimal_places)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_availability() {
        assert_eq!(availability(100.0), "100%");
        assert_eq!(availability(99.99982), "99.9998%");
        assert_eq!(availability(99.981), "99.98%");
        assert_eq!(availability(99.912), "99.91%");
        assert_eq!(availability(99.5123), "99.5%");
        assert_eq!(availability(90.23), "90%");
        assert_eq!(availability(0.15), "0%");
    }
}