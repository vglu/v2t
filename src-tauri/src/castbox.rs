//! Castbox channel → episode list (podcast expand).
//!
//! Castbox channel pages are not playlists for yt-dlp (`Unsupported URL`).
//! Public Everest APIs still expose the full episode index:
//! - `GET /data/episodes/overview?cids={cid}&web=1` → all eids
//! - `GET /data/episodes/v2?eids=…&web=1` → titles + media URLs (batched)

use serde::{Deserialize, Serialize};
use serde_json::Value;

const EVEREST: &str = "https://everest.castbox.fm";
const DETAIL_BATCH: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CastboxEpisode {
    pub eid: u64,
    pub title: String,
    /// Prefer direct media URL; fall back to Castbox episode page (yt-dlp generic).
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_date: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CastboxChannel {
    pub cid: u64,
    pub title: String,
    pub episodes: Vec<CastboxEpisode>,
}

/// `castbox.fm/channel/...-id2374602`, `/channel/id2374602`, `/ch/2374602`.
pub fn parse_channel_cid(url: &str) -> Option<u64> {
    let lower = url.trim().to_ascii_lowercase();
    if !lower.contains("castbox.fm") {
        return None;
    }
    // Episode pages also contain `-id{cid}-id{eid}` — not a channel.
    if lower.contains("/episode/") {
        return None;
    }
    if !(lower.contains("/channel/") || lower.contains("/ch/")) {
        return None;
    }
    // Prefer trailing `-idNNNN` or `/idNNNN` / `/ch/NNNN`.
    let re_tail = regex_lite_cid_tail(&lower)?;
    Some(re_tail)
}

fn regex_lite_cid_tail(lower: &str) -> Option<u64> {
    // ...-id2374602 or .../id2374602 (with optional query)
    if let Some(i) = lower.rfind("-id") {
        let rest = &lower[i + 3..];
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            return digits.parse().ok();
        }
    }
    if let Some(i) = lower.rfind("/id") {
        let rest = &lower[i + 3..];
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            return digits.parse().ok();
        }
    }
    if let Some(i) = lower.rfind("/ch/") {
        let rest = &lower[i + 4..];
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            return digits.parse().ok();
        }
    }
    None
}

pub fn is_castbox_channel_url(url: &str) -> bool {
    parse_channel_cid(url).is_some()
}

pub fn episode_page_url(cid: u64, eid: u64) -> String {
    format!("https://castbox.fm/episode/id{cid}-id{eid}")
}

/// Fetch full episode list for a Castbox channel URL.
pub async fn resolve_channel(url: &str) -> Result<CastboxChannel, String> {
    let cid = parse_channel_cid(url)
        .ok_or_else(|| "Not a Castbox channel URL (expected /channel/…-idNNNN)".to_string())?;
    resolve_channel_by_cid(cid).await
}

pub async fn resolve_channel_by_cid(cid: u64) -> Result<CastboxChannel, String> {
    let client = reqwest::Client::builder()
        .user_agent("v2t/castbox-expand")
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| format!("HTTP client: {e}"))?;

    let overview_url = format!("{EVEREST}/data/episodes/overview?cids={cid}&web=1");
    let overview_val = get_json(&client, &overview_url).await?;
    let (eids, dates) = parse_overview_eids(&overview_val, cid)?;
    if eids.is_empty() {
        return Err(format!("Castbox channel {cid}: no episodes in overview"));
    }

    let mut title = format!("Castbox channel {cid}");
    let mut by_eid: std::collections::HashMap<u64, (String, String)> =
        std::collections::HashMap::new();

    for chunk in eids.chunks(DETAIL_BATCH) {
        let joined = chunk
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let detail_url = format!("{EVEREST}/data/episodes/v2?eids={joined}&web=1");
        match get_json(&client, &detail_url).await {
            Ok(val) => {
                if let Some(ch_title) = channel_title_from_details(&val) {
                    title = ch_title;
                }
                for (eid, ep_title, media) in parse_episode_details(&val) {
                    by_eid.insert(eid, (ep_title, media));
                }
            }
            Err(e) => {
                // Titles optional — still emit episode page URLs.
                eprintln!("castbox detail batch failed: {e}");
            }
        }
    }

    let episodes = eids
        .iter()
        .enumerate()
        .map(|(i, eid)| {
            let (ep_title, media) = by_eid
                .get(eid)
                .cloned()
                .unwrap_or_else(|| (format!("Episode {eid}"), String::new()));
            let url = if media.trim().is_empty() {
                episode_page_url(cid, *eid)
            } else {
                media
            };
            CastboxEpisode {
                eid: *eid,
                title: ep_title,
                url,
                release_date: dates.get(i).cloned().flatten(),
            }
        })
        .collect();

    Ok(CastboxChannel {
        cid,
        title,
        episodes,
    })
}

async fn get_json(client: &reqwest::Client, url: &str) -> Result<Value, String> {
    let resp = client
        .get(url)
        .header("Accept", "application/json")
        .header("Referer", "https://castbox.fm/")
        .send()
        .await
        .map_err(|e| format!("Castbox request failed: {e}"))?;
    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("Castbox read body: {e}"))?;
    if !status.is_success() {
        return Err(format!(
            "Castbox HTTP {status}: {}",
            text.chars().take(200).collect::<String>()
        ));
    }
    serde_json::from_str(&text).map_err(|e| format!("Castbox JSON: {e}"))
}

fn parse_overview_eids(
    val: &Value,
    cid: u64,
) -> Result<(Vec<u64>, Vec<Option<String>>), String> {
    let data = val
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or_else(|| "Castbox overview: missing data[]".to_string())?;
    let entry = data
        .iter()
        .find(|e| e.get("cid").and_then(|c| c.as_u64()) == Some(cid))
        .or_else(|| data.first())
        .ok_or_else(|| "Castbox overview: empty data".to_string())?;
    let list = entry
        .get("episode_list")
        .and_then(|l| l.as_array())
        .ok_or_else(|| "Castbox overview: missing episode_list".to_string())?;

    let mut eids = Vec::with_capacity(list.len());
    let mut dates = Vec::with_capacity(list.len());
    for item in list {
        let Some(eid) = item.get("eid").and_then(|e| e.as_u64()) else {
            continue;
        };
        eids.push(eid);
        dates.push(
            item.get("release_date")
                .and_then(|d| d.as_str())
                .map(str::to_string),
        );
    }
    Ok((eids, dates))
}

fn channel_title_from_details(val: &Value) -> Option<String> {
    let arr = val.get("data")?.as_array()?;
    let first = arr.first()?;
    first
        .get("channel")
        .and_then(|c| c.get("title"))
        .and_then(|t| t.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn parse_episode_details(val: &Value) -> Vec<(u64, String, String)> {
    let Some(arr) = val.get("data").and_then(|d| d.as_array()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for item in arr {
        let Some(eid) = item.get("eid").and_then(|e| e.as_u64()) else {
            continue;
        };
        let title = item
            .get("title")
            .and_then(|t| t.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("Episode")
            .to_string();
        let media = item
            .get("url")
            .and_then(|u| u.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .or_else(|| {
                item.get("urls")
                    .and_then(|u| u.as_array())
                    .and_then(|a| a.first())
                    .and_then(|u| u.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
            })
            .unwrap_or("")
            .to_string();
        out.push((eid, title, media));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_channel_from_slug_url() {
        let cid = parse_channel_cid(
            "https://castbox.fm/channel/%D0%A5%D0%BE%D1%87%D1%83-id2374602?country=us",
        );
        assert_eq!(cid, Some(2374602));
    }

    #[test]
    fn parse_channel_from_id_form() {
        assert_eq!(
            parse_channel_cid("https://castbox.fm/channel/id2374602"),
            Some(2374602)
        );
        assert_eq!(
            parse_channel_cid("https://castbox.fm/ch/2374602"),
            Some(2374602)
        );
    }

    #[test]
    fn episode_url_is_not_channel() {
        assert!(parse_channel_cid("https://castbox.fm/episode/id2374602-id653290517").is_none());
    }

    #[test]
    fn parse_overview_sample() {
        let json = r#"{
          "msg":"OK","code":0,
          "data":[{
            "cid":2374602,
            "latest_eid":1,
            "episode_list":[
              {"release_date":"2023-12-03T08:30:00Z","eid":653487215},
              {"release_date":"2023-12-02T08:30:00Z","eid":653290519}
            ]
          }]
        }"#;
        let val: Value = serde_json::from_str(json).unwrap();
        let (eids, dates) = parse_overview_eids(&val, 2374602).unwrap();
        assert_eq!(eids, vec![653487215, 653290519]);
        assert_eq!(dates[0].as_deref(), Some("2023-12-03T08:30:00Z"));
    }

    #[test]
    fn parse_details_sample() {
        let json = r#"{
          "msg":"OK","code":0,
          "data":[{
            "eid":653487215,
            "title":"Геометрия",
            "url":"https://cdn.example/a.mp3",
            "channel":{"title":"Хочу всё знать","cid":2374602}
          }]
        }"#;
        let val: Value = serde_json::from_str(json).unwrap();
        assert_eq!(channel_title_from_details(&val).as_deref(), Some("Хочу всё знать"));
        let eps = parse_episode_details(&val);
        assert_eq!(eps.len(), 1);
        assert_eq!(eps[0].0, 653487215);
        assert_eq!(eps[0].1, "Геометрия");
        assert!(eps[0].2.ends_with(".mp3"));
    }
}
