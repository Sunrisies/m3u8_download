// #[cfg(test)]
use m3u8_rs::MediaSegment;
use m3u8_rs::Playlist;
use reqwest::blocking::get;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use url::Url;

/// 定义一个trait，用于获取播放列表中的片段
pub trait PlaylistExt {
    /// 获取播放列表中的所有片段
    fn get_segments(&self) -> Option<Vec<MediaSegment>>;

    /// 获取指定索引范围的片段
    fn get_segments_range(&self, start: usize, end: usize) -> Option<Vec<MediaSegment>>;

    /// 获取从指定索引开始到结尾的所有片段
    fn get_segments_from(&self, start: usize) -> Option<Vec<MediaSegment>>;

    /// 获取从开头到指定索引的所有片段
    fn get_segments_to(&self, end: usize) -> Option<Vec<MediaSegment>>;

    /// 根据URI获取片段
    fn get_segment_by_uri(&self, uri: &str) -> Option<MediaSegment>;

    /// 根据时间偏移获取片段
    fn get_segment_by_time_offset(&self, time_offset: f32) -> Option<MediaSegment>;

    /// 获取包含指定时间点的片段及其周围的片段
    fn get_segments_around_time(
        &self,
        time_offset: f32,
        buffer_size: usize,
    ) -> Option<Vec<MediaSegment>>;
}

/// 为Playlist实现PlaylistExt trait
impl PlaylistExt for Playlist {
    /// 获取播放列表中的所有片段
    fn get_segments(&self) -> Option<Vec<MediaSegment>> {
        match self {
            Playlist::MediaPlaylist(media) => Some(media.segments.clone()),
            Playlist::MasterPlaylist(_) => None, // 主播放列表不包含片段
        }
    }

    /// 获取指定索引范围的片段
    fn get_segments_range(&self, start: usize, end: usize) -> Option<Vec<MediaSegment>> {
        match self {
            Playlist::MediaPlaylist(media) => {
                let segments = &media.segments;
                if start >= segments.len() || end > segments.len() || start > end {
                    return None;
                }
                Some(segments[start..end].to_vec())
            }
            Playlist::MasterPlaylist(_) => None,
        }
    }

    /// 获取从指定索引开始到结尾的所有片段
    fn get_segments_from(&self, start: usize) -> Option<Vec<MediaSegment>> {
        match self {
            Playlist::MediaPlaylist(media) => {
                let segments = &media.segments;
                if start >= segments.len() {
                    return None;
                }
                Some(segments[start..].to_vec())
            }
            Playlist::MasterPlaylist(_) => None,
        }
    }

    /// 获取从开头到指定索引的所有片段
    fn get_segments_to(&self, end: usize) -> Option<Vec<MediaSegment>> {
        match self {
            Playlist::MediaPlaylist(media) => {
                let segments = &media.segments;
                if end > segments.len() {
                    return None;
                }
                Some(segments[..end].to_vec())
            }
            Playlist::MasterPlaylist(_) => None,
        }
    }

    /// 根据URI获取片段
    fn get_segment_by_uri(&self, uri: &str) -> Option<MediaSegment> {
        match self {
            Playlist::MediaPlaylist(media) => media.segments.iter().find(|s| s.uri == uri).cloned(),
            Playlist::MasterPlaylist(_) => None,
        }
    }

    /// 根据时间偏移获取片段
    fn get_segment_by_time_offset(&self, time_offset: f32) -> Option<MediaSegment> {
        match self {
            Playlist::MediaPlaylist(media) => {
                let mut accumulated_time = 0.0;

                for segment in &media.segments {
                    accumulated_time += segment.duration;
                    if accumulated_time >= time_offset {
                        return Some(segment.clone());
                    }
                }

                None
            }
            Playlist::MasterPlaylist(_) => None,
        }
    }

    /// 获取包含指定时间点的片段及其周围的片段
    fn get_segments_around_time(
        &self,
        time_offset: f32,
        buffer_size: usize,
    ) -> Option<Vec<MediaSegment>> {
        match self {
            Playlist::MediaPlaylist(media) => {
                let mut accumulated_time = 0.0;
                let mut segment_index = 0;

                // 找到包含时间偏移的片段
                for (i, segment) in media.segments.iter().enumerate() {
                    accumulated_time += segment.duration;
                    if accumulated_time >= time_offset {
                        segment_index = i;
                        break;
                    }
                }

                // 计算起始和结束索引
                let start = if segment_index >= buffer_size {
                    segment_index - buffer_size
                } else {
                    0
                };

                let end = if segment_index + buffer_size + 1 <= media.segments.len() {
                    segment_index + buffer_size + 1
                } else {
                    media.segments.len()
                };

                Some(media.segments[start..end].to_vec())
            }
            Playlist::MasterPlaylist(_) => None,
        }
    }
}

/// 从本地文件路径或远程URL加载M3U8内容
fn load_m3u8_content(source: &str) -> Result<String, String> {
    // 检查是否是URL
    if let Ok(parsed_url) = Url::parse(source) {
        if parsed_url.scheme() == "http" || parsed_url.scheme() == "https" {
            // 从远程URL加载
            match get(source) {
                Ok(response) => {
                    if response.status().is_success() {
                        match response.text() {
                            Ok(text) => Ok(text),
                            Err(e) => Err(format!("Failed to read response text: {}", e)),
                        }
                    } else {
                        Err(format!(
                            "HTTP request failed with status: {}",
                            response.status()
                        ))
                    }
                }
                Err(e) => Err(format!("Failed to fetch URL: {}", e)),
            }
        } else {
            // 可能是本地文件路径
            load_from_file(source)
        }
    } else {
        // 不是有效的URL，尝试作为本地文件路径处理
        load_from_file(source)
    }
}

/// 从本地文件加载内容
fn load_from_file(path: &str) -> Result<String, String> {
    let path = Path::new(path);
    if !path.exists() {
        return Err(format!("File does not exist: {}", path.display()));
    }

    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(e) => return Err(format!("Failed to open file: {}", e)),
    };

    let mut content = String::new();
    if let Err(e) = file.read_to_string(&mut content) {
        return Err(format!("Failed to read file: {}", e));
    }

    Ok(content)
}

/// 解析M3U8内容
fn parse_m3u8(content: &str) -> Result<Playlist, String> {
    match m3u8_rs::parse_playlist(content.as_bytes()) {
        Ok((_, playlist)) => Ok(playlist),
        Err(e) => Err(format!("Failed to parse M3U8: {:?}", e)),
    }
}

/// 主函数 - 从源加载并解析M3U8
pub fn parse_m3u8_from_source(source: &str) -> Result<Playlist, String> {
    // 加载内容
    let content = load_m3u8_content(source)?;

    // 解析内容
    let playlist = parse_m3u8(&content)?;
    match playlist {
        Playlist::MasterPlaylist(_) => println!("主播放列表"),
        Playlist::MediaPlaylist(_) => println!("媒体播放列表"),
    }
    Ok(playlist)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_m3u8_from_url() {
        // 使用一个公开的M3U8测试URL
        let url = "https://playertest.longtailvideo.com/adaptive/oceans_aes/oceans_aes.m3u8";
        let result = parse_m3u8_from_source(url);

        assert!(
            result.is_ok(),
            "Failed to parse M3U8 from URL: {:?}",
            result.err()
        );

        let playlist = result.unwrap();
        println!("Parsed playlist from URL: {:?}", playlist);
    }

    #[test]
    fn test_parse_m3u8_from_file() {
        // 这个测试需要一个实际的M3U8文件
        // 在实际使用中，你需要提供一个存在的文件路径
        let path = "test.m3u8";

        // 创建一个测试文件
        let test_content = "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:10\n#EXTINF:10.0,\nsegment1.ts\n#EXTINF:10.0,\nsegment2.ts\n#EXT-X-ENDLIST";
        std::fs::write(path, test_content).expect("Failed to create test file");

        let result = parse_m3u8_from_source(path);

        assert!(
            result.is_ok(),
            "Failed to parse M3U8 from file: {:?}",
            result.err()
        );

        let playlist = result.unwrap();
        println!("Parsed playlist from file: {:?}", playlist);

        // 清理测试文件
        std::fs::remove_file(path).expect("Failed to remove test file");
    }

    #[test]
    fn test_get_segments() {
        // 创建一个测试媒体播放列表
        let path = "test_segments.m3u8";
        let test_content = "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:10\n#EXTINF:10.0,\nsegment1.ts\n#EXTINF:10.0,\nsegment2.ts\n#EXTINF:10.0,\nsegment3.ts\n#EXT-X-ENDLIST";
        std::fs::write(path, test_content).expect("Failed to create test file");

        let playlist = parse_m3u8_from_source(path).unwrap();

        // 测试获取所有片段
        if let Some(segments) = playlist.get_segments() {
            assert_eq!(segments.len(), 3);
            println!("Total segments: {}", segments.len());
        } else {
            panic!("Failed to get segments");
        }

        // 测试获取指定索引范围的片段
        if let Some(segments) = playlist.get_segments_range(1, 3) {
            assert_eq!(segments.len(), 2);
            println!("Segments 1-3: {:?}", segments);
        } else {
            panic!("Failed to get segments range");
        }

        // 测试获取从指定索引开始到结尾的所有片段
        if let Some(segments) = playlist.get_segments_from(1) {
            assert_eq!(segments.len(), 2);
            println!("Segments from index 1: {:?}", segments);
        } else {
            panic!("Failed to get segments from index");
        }

        // 测试获取从开头到指定索引的所有片段
        if let Some(segments) = playlist.get_segments_to(2) {
            assert_eq!(segments.len(), 2);
            println!("Segments up to index 2: {:?}", segments);
        } else {
            panic!("Failed to get segments to index");
        }

        // 测试根据URI获取片段
        if let Some(segment) = playlist.get_segment_by_uri("segment2.ts") {
            assert_eq!(segment.uri, "segment2.ts");
            println!("Segment with URI 'segment2.ts': {:?}", segment);
        } else {
            panic!("Failed to get segment by URI");
        }

        // 测试根据时间偏移获取片段
        if let Some(segment) = playlist.get_segment_by_time_offset(15.0) {
            assert_eq!(segment.uri, "segment2.ts");
            println!("Segment at time offset 15s: {:?}", segment);
        } else {
            panic!("Failed to get segment by time offset");
        }

        // 测试获取包含指定时间点的片段及其周围的片段
        if let Some(segments) = playlist.get_segments_around_time(15.0, 1) {
            assert_eq!(segments.len(), 3);
            println!("Segments around time offset 15s: {:?}", segments);
        } else {
            panic!("Failed to get segments around time");
        }

        // 清理测试文件
        std::fs::remove_file(path).expect("Failed to remove test file");
    }
}
