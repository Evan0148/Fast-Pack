#[cfg(feature = "compact-data")]
use std::io::Read;

pub(crate) fn text(data: &[u8]) -> String {
    #[cfg(not(feature = "compact-data"))]
    {
        String::from_utf8(data.to_vec()).expect("内置资源编码无效")
    }
    #[cfg(feature = "compact-data")]
    {
        let mut text = String::new();
        flate2::read::DeflateDecoder::new(data)
            .read_to_string(&mut text)
            .expect("压缩的内置资源无效");
        text
    }
}
