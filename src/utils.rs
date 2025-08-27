pub trait Elide {
    type Output;
    fn elide(&self, len: usize) -> Self::Output;
}

impl Elide for String {
    type Output = String;
    fn elide(&self, len: usize) -> Self::Output {
        if self.len() > len {
            format!("{}...", &self[..len-3])
        } else {
            self.clone()
        }
    }
}

impl Elide for &str {
    type Output = String;
    fn elide(&self, len: usize) -> Self::Output {
        if self.len() > len {
            format!("{}...", &self[..len-3])
        } else {
            self.to_string()
        }
    }
}