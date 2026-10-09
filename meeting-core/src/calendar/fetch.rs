//! F18-FR3: fetching a link. The HTTP lives in the providers module (the only one that names
//! the HTTP library, R8); this is its calendar-side name.
pub use crate::providers::calendar::{
    CONNECT_TIMEOUT, FetchCfg, Fetched, GONE_TEXT, MAX_BYTES, MAX_REDIRECTS, REQUEST_TIMEOUT,
    Validators, fetch, gunzip, next_hop,
};
