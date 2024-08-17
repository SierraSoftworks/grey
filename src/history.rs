pub struct History<const N: usize, T: Sized> {
    records: [Option<T>; N],
    index: usize,
}
