#[derive(Clone, Copy, PartialEq)]
pub enum View {
    Overview,
    Settings,
    System(u8),
}
