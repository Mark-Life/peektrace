//! JavaScript semantics the TS CLI output depends on: number and date
//! formatting, `Date.parse`, `JSON.parse` leniency, string trimming and
//! ordering.

pub mod json;
pub mod json_out;
pub mod num;
pub mod os;
pub mod regex;
pub mod system_zone;
pub mod text;
pub mod time;
pub mod units;
pub mod value;
