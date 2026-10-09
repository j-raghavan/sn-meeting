//! F16: the spend calls. Rates (F16-FR4) are read from and written to `settings.json`.
use super::App;
use crate::rates::{self, Rate, RateTable};
use chrono::NaiveDate;
use serde_json::{Value, json};

impl App {
    /// The rates in force now: the defaults with the user's entries over them.
    pub fn rates(&self) -> RateTable {
        RateTable::effective(&self.settings().0.rates)
    }

    /// Today in the device's time zone.
    pub fn today(&self) -> NaiveDate {
        let now = chrono::Utc::now().timestamp_millis();
        crate::tz::local_at(self.tz.as_ref(), now)
            .map(|t| t.date_naive())
            .unwrap_or_default()
    }

    /// `nativeGetRates`: `{asOf, entries: [Rate + origin], problems: [line]}`.
    pub fn get_rates_json(&self) -> String {
        let t = self.rates();
        let entries: Vec<Value> = t
            .entries
            .iter()
            .map(|(r, origin)| {
                let mut v = json!(r);
                v["origin"] = json!(origin);
                v
            })
            .collect();
        json!({"asOf": t.as_of(), "entries": entries, "problems": t.problems}).to_string()
    }

    /// `nativeSetRates(json)`: the user's whole list of entries (an array, or `{entries}`),
    /// replacing the stored one; `[]` is "Reset to defaults". An entry that charges what its
    /// default charges is not stored; one unchanged from the stored entry keeps its `asOf`; any
    /// other is dated today. Replies as `nativeGetRates`, or `{error, field}` and nothing is
    /// written. Stored costs are not touched (F16-FR6).
    pub fn set_rates_json(&self, input: &str) -> String {
        let Ok(v) = serde_json::from_str::<Value>(input) else {
            return json!({"error": "rates must be JSON", "field": null}).to_string();
        };
        let list = match v.get("entries").unwrap_or(&v) {
            Value::Array(a) => a.clone(),
            _ => return json!({"error": "rates must be a list", "field": null}).to_string(),
        };
        let today = self.today().format("%Y-%m-%d").to_string();
        let r = self.commit_settings(|cur| {
            let stored: Vec<Rate> = cur
                .rates
                .iter()
                .filter_map(|v| rates::parse_user(v).ok())
                .collect();
            let mut out = Vec::new();
            for (i, item) in list.iter().enumerate() {
                let mut item = item.clone();
                if let Some(o) = item.as_object_mut() {
                    o.remove("origin"); // as nativeGetRates lists it
                    o.entry("asOf").or_insert_with(|| json!(today));
                    o.entry("source")
                        .or_insert_with(|| json!(rates::USER_SOURCE));
                }
                let mut r: Rate = serde_json::from_value(item).map_err(|e| {
                    json!({"error": format!("rate {}: {e}", i + 1), "field": format!("rates[{i}]")})
                        .to_string()
                })?;
                if let Err((f, m)) = r.check() {
                    let field = format!("rates[{i}].{f}");
                    return Err(json!({"error": m, "field": field}).to_string());
                }
                if RateTable::default_for(&r).is_some_and(|d| d.same_prices(&r)) {
                    continue;
                }
                match stored
                    .iter()
                    .find(|s| s.key() == r.key() && s.same_prices(&r))
                {
                    Some(s) => r = s.clone(),
                    None => {
                        r.as_of = today.clone();
                        r.source = rates::USER_SOURCE.to_owned();
                    }
                }
                out.push(json!(r));
            }
            let mut next = cur.clone();
            next.rates = out;
            Ok(next)
        });
        match r {
            Ok(()) => self.get_rates_json(),
            Err(e) => e,
        }
    }
}
