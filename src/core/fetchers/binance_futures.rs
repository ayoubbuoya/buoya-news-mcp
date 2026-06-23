//! Crypto-futures derivatives metrics from Binance's keyless USDⓈ-M public API.
//!
//! For each configured symbol we read three endpoints — open interest, the
//! premium index (funding rate + mark price), and the global long/short account
//! ratio — and fold them into one [`DerivativesSnapshot`]. These are the numbers
//! market makers watch; they're stored structured in the `derivatives` table, not
//! as text articles.
//!
//! Every metric is best-effort: a single endpoint failing for one symbol logs a
//! warning and leaves that field `None` rather than dropping the whole reading.
//! Parsing is split out from the HTTP calls (the `parse_*` fns) so it can be tested
//! against fixtures without network access.

use chrono::{DateTime, TimeZone, Utc};
use serde::Deserialize;

use crate::core::{error::FetchError, types::DerivativesSnapshot};

const FAPI_BASE: &str = "https://fapi.binance.com";

/// Binance `/fapi/v1/openInterest`: numbers come back as strings.
#[derive(Debug, Deserialize)]
struct OpenInterestResp {
    #[serde(rename = "openInterest")]
    open_interest: String,
}

/// Binance `/fapi/v1/premiumIndex`.
#[derive(Debug, Deserialize)]
struct PremiumIndexResp {
    #[serde(rename = "markPrice")]
    mark_price: String,
    #[serde(rename = "lastFundingRate")]
    last_funding_rate: String,
    #[serde(rename = "nextFundingTime")]
    next_funding_time: i64,
}

/// One element of an account/position long-short ratio series. Shared shape across
/// Binance's `globalLongShortAccountRatio` (retail crowd) and
/// `topLongShortPositionRatio` (largest traders by position).
#[derive(Debug, Deserialize)]
struct AccountRatioResp {
    #[serde(rename = "longShortRatio")]
    long_short_ratio: String,
    #[serde(rename = "longAccount")]
    long_account: String,
    #[serde(rename = "shortAccount")]
    short_account: String,
}

/// One element of Binance `/futures/data/takerlongshortRatio`.
#[derive(Debug, Deserialize)]
struct TakerRatioResp {
    #[serde(rename = "buySellRatio")]
    buy_sell_ratio: String,
    #[serde(rename = "buyVol")]
    buy_vol: String,
    #[serde(rename = "sellVol")]
    sell_vol: String,
}

fn parse_open_interest(bytes: &[u8]) -> Result<f64, FetchError> {
    let resp: OpenInterestResp =
        serde_json::from_slice(bytes).map_err(|e| FetchError::Parse(e.to_string()))?;
    resp.open_interest
        .parse()
        .map_err(|e| FetchError::Parse(format!("openInterest not a number: {e}")))
}

/// Returns `(mark_price, funding_rate, next_funding_time)`.
fn parse_premium_index(bytes: &[u8]) -> Result<(f64, f64, Option<DateTime<Utc>>), FetchError> {
    let resp: PremiumIndexResp =
        serde_json::from_slice(bytes).map_err(|e| FetchError::Parse(e.to_string()))?;
    let mark_price = parse_field(&resp.mark_price, "markPrice")?;
    let funding_rate = parse_field(&resp.last_funding_rate, "lastFundingRate")?;
    // 0 means "no next funding scheduled"; treat as absent rather than the epoch.
    let next = if resp.next_funding_time > 0 {
        Utc.timestamp_millis_opt(resp.next_funding_time).single()
    } else {
        None
    };
    Ok((mark_price, funding_rate, next))
}

/// Parse a number that Binance encodes as a JSON string, naming the field in any
/// error.
fn parse_field(raw: &str, field: &str) -> Result<f64, FetchError> {
    raw.parse()
        .map_err(|e| FetchError::Parse(format!("{field} not a number: {e}")))
}

/// Returns `(long_short_ratio, long_account, short_account)` from the most recent
/// (last) element of a long/short series. An empty array yields `None`. Shared by
/// the global-account and top-trader-position endpoints.
#[allow(clippy::type_complexity)]
fn parse_account_ratio(bytes: &[u8]) -> Result<Option<(f64, f64, f64)>, FetchError> {
    let series: Vec<AccountRatioResp> =
        serde_json::from_slice(bytes).map_err(|e| FetchError::Parse(e.to_string()))?;
    let Some(latest) = series.last() else {
        return Ok(None);
    };
    Ok(Some((
        parse_field(&latest.long_short_ratio, "longShortRatio")?,
        parse_field(&latest.long_account, "longAccount")?,
        parse_field(&latest.short_account, "shortAccount")?,
    )))
}

/// Returns `(buy_sell_ratio, buy_vol, sell_vol)` from the most recent (last)
/// element of the taker-volume series. An empty array yields `None`.
#[allow(clippy::type_complexity)]
fn parse_taker_ratio(bytes: &[u8]) -> Result<Option<(f64, f64, f64)>, FetchError> {
    let series: Vec<TakerRatioResp> =
        serde_json::from_slice(bytes).map_err(|e| FetchError::Parse(e.to_string()))?;
    let Some(latest) = series.last() else {
        return Ok(None);
    };
    Ok(Some((
        parse_field(&latest.buy_sell_ratio, "buySellRatio")?,
        parse_field(&latest.buy_vol, "buyVol")?,
        parse_field(&latest.sell_vol, "sellVol")?,
    )))
}

/// GET a URL and return the body bytes, mapping transport/status errors.
async fn get_bytes(http_client: &reqwest::Client, url: &str) -> Result<Vec<u8>, FetchError> {
    let bytes = http_client
        .get(url)
        .send()
        .await
        .map_err(FetchError::Http)?
        .error_for_status()
        .map_err(FetchError::Http)?
        .bytes()
        .await
        .map_err(FetchError::Http)?;
    Ok(bytes.to_vec())
}

/// GET `url`, parse the body with `parse`, and reduce both transport and parse
/// failures to `None` after logging — so one endpoint failing for a symbol leaves
/// just that metric absent. `metric` names the field in log lines.
async fn fetch_parse<T>(
    http_client: &reqwest::Client,
    url: &str,
    symbol: &str,
    metric: &str,
    parse: fn(&[u8]) -> Result<T, FetchError>,
) -> Option<T> {
    match get_bytes(http_client, url).await {
        Ok(bytes) => parse(&bytes)
            .map_err(|e| tracing::warn!("derivatives {symbol}: {metric}: {e}"))
            .ok(),
        Err(e) => {
            tracing::warn!("derivatives {symbol}: {metric} request: {e}");
            None
        }
    }
}

/// Fetch a derivatives snapshot for one symbol. The five endpoints are read
/// independently and each failure is tolerated (logged, field left `None`), so a
/// partial outage still yields a useful row. Returns `None` only when *every*
/// metric failed — nothing worth storing.
async fn fetch_one(
    http_client: &reqwest::Client,
    symbol: &str,
    period: &str,
) -> Option<DerivativesSnapshot> {
    let oi_url = format!("{FAPI_BASE}/fapi/v1/openInterest?symbol={symbol}");
    let premium_url = format!("{FAPI_BASE}/fapi/v1/premiumIndex?symbol={symbol}");
    let ls_url = format!(
        "{FAPI_BASE}/futures/data/globalLongShortAccountRatio?symbol={symbol}&period={period}&limit=1"
    );
    let taker_url = format!(
        "{FAPI_BASE}/futures/data/takerlongshortRatio?symbol={symbol}&period={period}&limit=1"
    );
    let top_url = format!(
        "{FAPI_BASE}/futures/data/topLongShortPositionRatio?symbol={symbol}&period={period}&limit=1"
    );

    let open_interest =
        fetch_parse(http_client, &oi_url, symbol, "open interest", parse_open_interest).await;

    let (mark_price, funding_rate, next_funding_time) =
        match fetch_parse(http_client, &premium_url, symbol, "premium index", parse_premium_index)
            .await
        {
            Some((m, f, n)) => (Some(m), Some(f), n),
            None => (None, None, None),
        };

    let (long_short_ratio, long_account, short_account) =
        match fetch_parse(http_client, &ls_url, symbol, "long/short ratio", parse_account_ratio)
            .await
            .flatten()
        {
            Some((r, l, s)) => (Some(r), Some(l), Some(s)),
            None => (None, None, None),
        };

    let (taker_buy_sell_ratio, taker_buy_vol, taker_sell_vol) =
        match fetch_parse(http_client, &taker_url, symbol, "taker volume", parse_taker_ratio)
            .await
            .flatten()
        {
            Some((r, b, s)) => (Some(r), Some(b), Some(s)),
            None => (None, None, None),
        };

    let (top_trader_long_short_ratio, top_trader_long_account, top_trader_short_account) =
        match fetch_parse(http_client, &top_url, symbol, "top-trader ratio", parse_account_ratio)
            .await
            .flatten()
        {
            Some((r, l, s)) => (Some(r), Some(l), Some(s)),
            None => (None, None, None),
        };

    // Nothing came back at all — skip the symbol this tick.
    if open_interest.is_none()
        && mark_price.is_none()
        && funding_rate.is_none()
        && long_short_ratio.is_none()
        && taker_buy_sell_ratio.is_none()
        && top_trader_long_short_ratio.is_none()
    {
        return None;
    }

    let open_interest_usd = match (open_interest, mark_price) {
        (Some(oi), Some(mp)) => Some(oi * mp),
        _ => None,
    };

    Some(DerivativesSnapshot {
        symbol: symbol.to_string(),
        open_interest,
        open_interest_usd,
        funding_rate,
        mark_price,
        long_short_ratio,
        long_account,
        short_account,
        taker_buy_sell_ratio,
        taker_buy_vol,
        taker_sell_vol,
        top_trader_long_short_ratio,
        top_trader_long_account,
        top_trader_short_account,
        next_funding_time,
    })
}

/// Fetch a derivatives snapshot for each configured symbol. Symbols are fetched
/// sequentially to stay polite to Binance's keyless rate limits; symbols that
/// return nothing are simply omitted from the result.
pub async fn fetch_derivatives(
    http_client: &reqwest::Client,
    symbols: &[String],
    period: &str,
) -> Vec<DerivativesSnapshot> {
    let mut out = Vec::with_capacity(symbols.len());
    for symbol in symbols {
        if let Some(snapshot) = fetch_one(http_client, symbol, period).await {
            out.push(snapshot);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_open_interest() {
        let body = br#"{"symbol":"HBARUSDT","openInterest":"1234567.8","time":1700000000000}"#;
        assert_eq!(parse_open_interest(body).unwrap(), 1234567.8);
    }

    #[test]
    fn parses_premium_index() {
        let body = br#"{"symbol":"BTCUSDT","markPrice":"64250.50","indexPrice":"64200.0",
            "lastFundingRate":"0.0001","nextFundingTime":1700000000000,"time":1699999000000}"#;
        let (mark, funding, next) = parse_premium_index(body).unwrap();
        assert_eq!(mark, 64250.50);
        assert_eq!(funding, 0.0001);
        assert!(next.is_some());
    }

    #[test]
    fn premium_index_zero_next_funding_is_none() {
        let body = br#"{"markPrice":"1.0","lastFundingRate":"0.0","nextFundingTime":0}"#;
        let (_, _, next) = parse_premium_index(body).unwrap();
        assert!(next.is_none());
    }

    #[test]
    fn parses_account_ratio_taking_latest() {
        let body = br#"[
            {"symbol":"ETHUSDT","longShortRatio":"1.20","longAccount":"0.55","shortAccount":"0.45","timestamp":1},
            {"symbol":"ETHUSDT","longShortRatio":"1.50","longAccount":"0.60","shortAccount":"0.40","timestamp":2}
        ]"#;
        let (ratio, long, short) = parse_account_ratio(body).unwrap().unwrap();
        assert_eq!(ratio, 1.50);
        assert_eq!(long, 0.60);
        assert_eq!(short, 0.40);
    }

    #[test]
    fn empty_account_ratio_series_is_none() {
        assert!(parse_account_ratio(b"[]").unwrap().is_none());
    }

    #[test]
    fn parses_taker_ratio_taking_latest() {
        let body = br#"[
            {"buySellRatio":"0.90","buyVol":"100.0","sellVol":"110.0","timestamp":1},
            {"buySellRatio":"1.25","buyVol":"250.5","sellVol":"200.4","timestamp":2}
        ]"#;
        let (ratio, buy, sell) = parse_taker_ratio(body).unwrap().unwrap();
        assert_eq!(ratio, 1.25);
        assert_eq!(buy, 250.5);
        assert_eq!(sell, 200.4);
        assert!(parse_taker_ratio(b"[]").unwrap().is_none());
    }

    #[test]
    fn malformed_open_interest_errors() {
        assert!(parse_open_interest(b"not json").is_err());
        let bad_number = br#"{"openInterest":"abc"}"#;
        assert!(parse_open_interest(bad_number).is_err());
    }
}
