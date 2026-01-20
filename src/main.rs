mod utils;
use utils::{parse_m3u8_from_source, PlaylistExt};

fn main() {
    // 示例：从URL解析M3U8
    let url = "https://vip.ffzy-video.com/20260115/35584_339cb038/2000k/hls/mixed.m3u8";
    match parse_m3u8_from_source(url) {
        Ok(playlist) => {
            println!("Successfully parsed M3U8 from URL:");
            // println!("{:?}", playlist);
            if let Some(segments) = playlist.get_segments() {
                println!("Total segments: {:?}", segments);
            }
        }
        Err(e) => eprintln!("Error: {}", e),
    }

    // // 示例：从本地文件解析M3U8
    // let file_path = "example.m3u8";
    // match parse_m3u8_from_source(file_path) {
    //     Ok(playlist) => {
    //         println!("Successfully parsed M3U8 from file:");
    //         println!("{:?}", playlist);
    //     }
    //     Err(e) => eprintln!("Error: {}", e),
    // }
}
