use nautilus::provider::astra::client::{
    detect_image_dimensions, parse_dimensions, parse_quality, sanitize_filename,
};

#[test]
fn test_detect_image_dimensions_png() {
    let mut data = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]; // PNG signature
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x0D]); // IHDR chunk length
    data.extend_from_slice(b"IHDR");
    data.extend_from_slice(&1920u32.to_be_bytes()); // width
    data.extend_from_slice(&1080u32.to_be_bytes()); // height
    data.extend_from_slice(&[8, 6, 0, 0, 0]);

    assert_eq!(detect_image_dimensions(&data), Some((1920, 1080)));
}

#[test]
fn test_detect_image_dimensions_gif() {
    let mut data = b"GIF89a".to_vec();
    data.extend_from_slice(&800u16.to_le_bytes()); // width
    data.extend_from_slice(&600u16.to_le_bytes()); // height

    assert_eq!(detect_image_dimensions(&data), Some((800, 600)));
}

#[test]
fn test_detect_image_dimensions_jpeg() {
    let mut data = vec![0xFF, 0xD8]; // SOI
    // APP0 marker
    data.extend_from_slice(&[0xFF, 0xE0, 0x00, 0x10]);
    data.extend_from_slice(&[0u8; 14]);
    // SOF0 marker (baseline DCT)
    data.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08]);
    data.extend_from_slice(&6008u16.to_be_bytes()); // height
    data.extend_from_slice(&4429u16.to_be_bytes()); // width
    data.extend_from_slice(&[0x03, 0x01, 0x11, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01]);

    assert_eq!(detect_image_dimensions(&data), Some((4429, 6008)));
}

#[test]
fn test_parse_dimensions_and_quality() {
    assert_eq!(parse_dimensions("1920x1080", "", false), Some((1920, 1080)));
    assert_eq!(parse_dimensions("", "720p", true), Some((720, 1280)));
    assert_eq!(parse_quality("1080p", ""), 1080);
    assert_eq!(parse_quality("HD", ""), 1080);
    assert_eq!(sanitize_filename("test/file?name", "default"), "test_file_name");
}
