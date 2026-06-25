#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageExtent {
    pub offset: u32,
    pub size: u32,
}

pub trait PageSource {
    type Error;

    fn read_at(&self, offset: u32, out: &mut [u8]) -> Result<(), Self::Error>;

    fn read_page(&self, extent: &PageExtent, out: &mut [u8]) -> Result<(), Self::Error> {
        self.read_at(extent.offset, out)
    }
}
