use aes::Aes128;
use aes::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
use anyhow::{Result, anyhow};
type Aes128CbcDec = cbc::Decryptor<Aes128>;

/// 解密TS片段数据
pub fn decrypt_segment(data: Vec<u8>, key: &[u8], segment_index: usize) -> Result<Vec<u8>> {
    if key.len() != 16 {
        return Err(anyhow!("AES 密钥长度必须为 16 字节"));
    }

    // 使用片段索引作为IV（初始化向量）
    let mut iv = [0u8; 16];
    let iv_bytes = (segment_index as u128).to_be_bytes();
    iv.copy_from_slice(&iv_bytes);

    let cipher = Aes128CbcDec::new(key.into(), &iv.into());

    // 解密数据
    let mut decrypted = data.clone();
    let decrypted_data = cipher
        .decrypt_padded_mut::<Pkcs7>(&mut decrypted)
        .map_err(|e| anyhow!("解密失败: {:?}", e))?;

    Ok(decrypted_data.to_vec())
}

/// 从M3U8内容中提取加密密钥
pub async fn extract_encryption_key(
    m3u8_content: &str,
    client: &reqwest::Client,
    base_url: &url::Url,
) -> Result<Option<Vec<u8>>> {
    // 查找 EXT-X-KEY 标签
    for line in m3u8_content.lines() {
        if line.starts_with("#EXT-X-KEY:") {
            if let Some(uri_start) = line.find("URI=\"") {
                let uri_start = uri_start + 5; // "URI=\"的长度
                if let Some(uri_end) = line[uri_start..].find("\"") {
                    let key_uri = &line[uri_start..uri_start + uri_end];
                    return Ok(Some(download_key(client, base_url, key_uri).await?));
                }
            }
        }
    }
    Ok(None)
}

/// 下载密钥
async fn download_key(
    client: &reqwest::Client,
    base_url: &url::Url,
    key_uri: &str,
) -> Result<Vec<u8>> {
    let full_url = crate::utils::resolve_url(base_url, key_uri)?;
    let response = client.get(&full_url).send().await?;

    if !response.status().is_success() {
        return Err(anyhow!("密钥下载失败: {}", response.status()));
    }

    Ok(response.bytes().await?.to_vec())
}
