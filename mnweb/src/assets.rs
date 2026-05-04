//! Static assets baked into the binary so `mnweb` is a single, self-contained
//! executable.
pub const INDEX_HTML: &str = include_str!("../static/index.html");
pub const APP_CSS: &str = include_str!("../static/app.css");
pub const APP_JS: &str = include_str!("../static/app.js");
