//! v210 layout helpers shared by the CPU reference path and the GL elements.
//!
//! v210 packs 6 pixels of 10-bit 4:2:2 YCbCr into 4 little-endian 32-bit words.
//! Each word holds three 10-bit fields plus 2 padding bits:
//!
//! ```text
//! word 0:  Cb0 | Y0 | Cr0
//! word 1:  Y1  | Cb2 | Y2
//! word 2:  Cr2 | Y3 | Cb4
//! word 3:  Y4  | Cr4 | Y5
//! ```
//!
//! That word layout is bit-identical to a `GL_RGB10_A2` texel uploaded with
//! `GL_UNSIGNED_INT_2_10_10_10_REV` (`RGB10A2_LE` in GStreamer).

/// Bytes per v210 row. 48 pixels occupy 128 bytes (32 words).
pub fn stride(width: u32) -> usize {
    (width.div_ceil(48) * 128) as usize
}

/// Proxy texture width when the same bytes are reinterpreted as `RGB10A2_LE`.
pub fn proxy_width(width: u32) -> u32 {
    width.div_ceil(48) * 32
}

/// Frame size in bytes for a packed v210 image.
pub fn frame_size(width: u32, height: u32) -> usize {
    stride(width) * height as usize
}

/// Extract one 10-bit field from a packed word (`shift` is 0, 10, or 20).
#[inline]
pub fn field(word: u32, shift: u32) -> u16 {
    ((word >> shift) & 0x3ff) as u16
}

/// Pack three 10-bit fields into one word. Padding bits are written as zero.
#[inline]
pub fn pack_word(a: u16, b: u16, c: u16) -> u32 {
    (a as u32 & 0x3ff) | ((b as u32 & 0x3ff) << 10) | ((c as u32 & 0x3ff) << 20)
}

/// One 4:2:2 pixel as 10-bit Y, Cb, Cr.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ycbcr {
    pub y: u16,
    pub cb: u16,
    pub cr: u16,
}

/// Unpack six pixels from four v210 words. Padding bits are ignored.
pub fn unpack_group(words: [u32; 4]) -> [Ycbcr; 6] {
    let cb0 = field(words[0], 0);
    let y0 = field(words[0], 10);
    let cr0 = field(words[0], 20);

    let y1 = field(words[1], 0);
    let cb2 = field(words[1], 10);
    let y2 = field(words[1], 20);

    let cr2 = field(words[2], 0);
    let y3 = field(words[2], 10);
    let cb4 = field(words[2], 20);

    let y4 = field(words[3], 0);
    let cr4 = field(words[3], 10);
    let y5 = field(words[3], 20);

    [
        Ycbcr {
            y: y0,
            cb: cb0,
            cr: cr0,
        },
        Ycbcr {
            y: y1,
            cb: cb0,
            cr: cr0,
        },
        Ycbcr {
            y: y2,
            cb: cb2,
            cr: cr2,
        },
        Ycbcr {
            y: y3,
            cb: cb2,
            cr: cr2,
        },
        Ycbcr {
            y: y4,
            cb: cb4,
            cr: cr4,
        },
        Ycbcr {
            y: y5,
            cb: cb4,
            cr: cr4,
        },
    ]
}

/// Pack six pixels into four v210 words. Padding bits are zero.
pub fn pack_group(pixels: [Ycbcr; 6]) -> [u32; 4] {
    [
        pack_word(pixels[0].cb, pixels[0].y, pixels[0].cr),
        pack_word(pixels[1].y, pixels[2].cb, pixels[2].y),
        pack_word(pixels[2].cr, pixels[3].y, pixels[4].cb),
        pack_word(pixels[4].y, pixels[4].cr, pixels[5].y),
    ]
}

/// Unpack a whole v210 frame into tightly packed Y, Cb, Cr planes (`width * height` each).
pub fn unpack_frame(src: &[u8], width: u32, height: u32) -> (Vec<u16>, Vec<u16>, Vec<u16>) {
    let stride = stride(width);
    let n = (width * height) as usize;
    let mut y = vec![0u16; n];
    let mut cb = vec![0u16; n];
    let mut cr = vec![0u16; n];

    for row in 0..height as usize {
        let row_off = row * stride;
        let mut x = 0u32;
        let mut word = 0usize;
        while x < width {
            let mut words = [0u32; 4];
            for (i, w) in words.iter_mut().enumerate() {
                let o = row_off + (word + i) * 4;
                *w = u32::from_le_bytes([src[o], src[o + 1], src[o + 2], src[o + 3]]);
            }
            let group = unpack_group(words);
            for pix in group {
                if x >= width {
                    break;
                }
                let idx = row * width as usize + x as usize;
                y[idx] = pix.y;
                cb[idx] = pix.cb;
                cr[idx] = pix.cr;
                x += 1;
            }
            word += 4;
        }
    }

    (y, cb, cr)
}

/// Pack tightly packed Y/Cb/Cr planes into a v210 frame. Padding bits are zero.
pub fn pack_frame(y: &[u16], cb: &[u16], cr: &[u16], width: u32, height: u32) -> Vec<u8> {
    let stride = stride(width);
    let mut dst = vec![0u8; stride * height as usize];

    for row in 0..height as usize {
        let row_off = row * stride;
        let mut x = 0u32;
        let mut word = 0usize;
        while x < width {
            let mut pixels = [Ycbcr { y: 0, cb: 0, cr: 0 }; 6];
            for pix in &mut pixels {
                if x >= width {
                    break;
                }
                let idx = row * width as usize + x as usize;
                *pix = Ycbcr {
                    y: y[idx],
                    cb: cb[idx],
                    cr: cr[idx],
                };
                x += 1;
            }
            // 4:2:2: even-pixel chroma is authoritative for the pair.
            pixels[1].cb = pixels[0].cb;
            pixels[1].cr = pixels[0].cr;
            pixels[3].cb = pixels[2].cb;
            pixels[3].cr = pixels[2].cr;
            pixels[5].cb = pixels[4].cb;
            pixels[5].cr = pixels[4].cr;
            let words = pack_group(pixels);
            for (i, w) in words.iter().enumerate() {
                let o = row_off + (word + i) * 4;
                dst[o..o + 4].copy_from_slice(&w.to_le_bytes());
            }
            word += 4;
        }
    }

    dst
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stride_and_proxy_match_byte_count() {
        for width in [50, 720, 1280, 1920] {
            assert_eq!(proxy_width(width) as usize * 4, stride(width));
        }
        assert_eq!(stride(1920), 5120);
        assert_eq!(proxy_width(1920), 1280);
        assert_eq!(stride(1280), 3456);
        assert_eq!(proxy_width(50), 64);
    }

    #[test]
    fn padding_bits_ignored_on_unpack() {
        let mut words = pack_group([
            Ycbcr {
                y: 64,
                cb: 512,
                cr: 512,
            },
            Ycbcr {
                y: 940,
                cb: 512,
                cr: 512,
            },
            Ycbcr {
                y: 0,
                cb: 64,
                cr: 960,
            },
            Ycbcr {
                y: 1023,
                cb: 64,
                cr: 960,
            },
            Ycbcr {
                y: 512,
                cb: 1023,
                cr: 0,
            },
            Ycbcr {
                y: 1,
                cb: 1023,
                cr: 0,
            },
        ]);
        for w in &mut words {
            *w |= 0xC000_0000;
        }
        let pixels = unpack_group(words);
        assert_eq!(pixels[0].y, 64);
        assert_eq!(pixels[1].y, 940);
        assert_eq!(pixels[2].cb, 64);
        assert_eq!(pixels[2].cr, 960);
        assert_eq!(pixels[4].cb, 1023);
        assert_eq!(pixels[5].y, 1);
    }

    #[test]
    fn pack_writes_zero_padding() {
        let words = pack_group(
            [Ycbcr {
                y: 64,
                cb: 512,
                cr: 512,
            }; 6],
        );
        for w in words {
            assert_eq!(w & 0xC000_0000, 0);
        }
    }

    #[test]
    fn roundtrip_awkward_widths() {
        for width in [50u32, 720, 1280, 1920] {
            let height = 2u32;
            let n = (width * height) as usize;
            let y: Vec<u16> = (0..n).map(|i| ((i * 17) % 1024) as u16).collect();
            let cb: Vec<u16> = (0..n).map(|i| (64 + (i * 3) % 897) as u16).collect();
            let cr: Vec<u16> = (0..n).map(|i| (64 + (i * 5) % 897) as u16).collect();
            let packed = pack_frame(&y, &cb, &cr, width, height);
            assert_eq!(packed.len(), frame_size(width, height));
            let (y2, cb2, cr2) = unpack_frame(&packed, width, height);
            assert_eq!(y2, y);
            // Chroma is 4:2:2: odd columns share the even-column sample.
            for row in 0..height as usize {
                for x in 0..width as usize {
                    let idx = row * width as usize + x;
                    let even = row * width as usize + (x & !1);
                    assert_eq!(cb2[idx], cb[even]);
                    assert_eq!(cr2[idx], cr[even]);
                }
            }
        }
    }
}
