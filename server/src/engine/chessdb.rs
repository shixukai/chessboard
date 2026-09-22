use std::time::Duration;

use super::QueryResult;
use super::QueryState;

const URL: &str = "http://www.chessdb.cn/chessdb.php";
const REFER: &str = "https://www.chessdb.cn/query/";
const AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/60.0.3112.113 Safari/537.36";
const SOURCE_CHESSDB: &str = "云库";
pub async fn query(fen: &str, timeout: u64) -> QueryResult {
    let mut records = super::QueryResult::default();
    let resp = reqwest::Client::new()
        .get(URL)
        .header("User-Agent", AGENT)
        .header("Referer", REFER)
        .query(&[("action", "querypv"), ("board", fen)])
        .timeout(Duration::from_secs(timeout))
        .send()
        .await;
    match resp {
        Ok(resp) => {
            let Ok(text) = resp.text().await else {
                records.state = QueryState::ServerInternalError;
                return records;
            };
            let text = text.strip_suffix('\0').unwrap_or(&text);
            match text {
                "" | "unknown" => records.state = QueryState::NotResult,
                "invalid board" | "checkmate" | "stalemate" => records.state = QueryState::InvalidBoard,
                text => {
                    tracing::debug!("chessdb response: {}", text);
                    for pair in text.split(',') {
                        let mut parts = pair.split(':');
                        match parts.next().unwrap_or("") {
                            "score" => records.score = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0),
                            "depth" => records.depth = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0),
                            "pv" => {
                                if let Some(pv_text) = parts.next() {
                                    for pv in pv_text.split('|') {
                                        if !pv.is_empty() {
                                            records.pvs.push(pv.to_string());
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    if !records.pvs.is_empty() {
                        records.state = QueryState::Success;
                        records.source = SOURCE_CHESSDB.to_string();
                    } else {
                        records.state = QueryState::NotResult;
                    }
                }
            }
        }
        Err(e) => {
            tracing::warn!("chessdb query error: {}", e);
            records.state = QueryState::ServerInternalError;
        }
    };
    records
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_query() {
    let result = query("2baka3/9/n3b4/r1p1nr2p/4R4/2PR5/4P3P/4B1N2/4A4/2B1KA3 w", 10).await;
    println!("{:?}", result);
}
