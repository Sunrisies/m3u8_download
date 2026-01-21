mod utils;
use utils::{parse_m3u8_from_source, PlaylistExt};

use crate::utils::{download_and_merge_playlist, load_and_process_download_tasks};

fn main() {
    // 示例：从URL解析M3U8
    // let url = "https://vip.ffzy-video.com/20260115/35584_339cb038/2000k/hls/mixed.m3u8";
    // match parse_m3u8_from_source(url) {
    //     Ok(playlist) => {
    //         println!("Successfully parsed M3U8 from URL:");
    //         // println!("{:?}", playlist);
    //         if let Some(segments) = playlist.get_segments() {
    //             println!("Total segments: {:?}", segments);
    //         }
    //     }
    //     Err(e) => eprintln!("Error: {}", e),
    // }
    match load_and_process_download_tasks("./examples/download_tasks.json") {
        Ok(tasks) => println!("Successfully processed all download tasks:{:?}", tasks),
        Err(e) => eprintln!("Failed to process download tasks: {}", e),
    }
    // 或者直接下载并合并
    // match download_and_merge_playlist(
    //     "https://vip.ffzy-video.com/20260115/35584_339cb038/2000k/hls/mixed.m3u8",
    //     "output.ts",
    //     "./temp_segments",
    // ) {
    //     Ok(_) => println!("Successfully downloaded and merged playlist"),
    //     Err(e) => eprintln!("Failed to download and merge playlist: {}", e),
    // }
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
