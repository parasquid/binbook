use crate::CanonicalGray2;

const BAYER_4X4: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

#[must_use]
pub fn ordered_bw(gray: CanonicalGray2, x: usize, y: usize) -> bool {
    let cutoff = match gray {
        CanonicalGray2::Black => 16,
        CanonicalGray2::DarkGray => 8,
        CanonicalGray2::LightGray => 4,
        CanonicalGray2::White => 0,
    };
    BAYER_4X4[y % 4][x % 4] < cutoff
}
