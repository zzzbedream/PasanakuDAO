//! NEP-297 structured events under the `pasanaku` standard.

use near_sdk::env;
use near_sdk::serde_json::{Value, json};

use crate::model::{EVENT_STANDARD, EVENT_VERSION};

/// Emit one `EVENT_JSON:` log line. `data` is wrapped in the standard array.
pub fn emit(event: &str, data: Value) {
    let payload = json!({
        "standard": EVENT_STANDARD,
        "version": EVENT_VERSION,
        "event": event,
        "data": [data],
    });
    env::log_str(&format!("EVENT_JSON:{payload}"));
}
