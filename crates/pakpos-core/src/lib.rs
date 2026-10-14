//! # pakpos-core
//!
//! The engine behind PakPos, an open-source API client written in Rust.
//! It is UI-agnostic: the desktop app (`pakpos`) and the command-line runner
//! (`pakpos-cli`) are both thin layers over this crate.
//!
//! ```no_run
//! use pakpos_core::{http, model::{HttpMethod, Request}, vars::VarScope};
//!
//! let mut scope = VarScope::new();
//! scope.environment.insert("base".into(), "https://httpbin.org".into());
//! let req = Request::new(HttpMethod::Get, "{{base}}/get");
//! let (_prepared, resp) = http::execute(&req, &scope, None, &Default::default()).unwrap();
//! println!("{} in {} ms", resp.status, resp.elapsed_ms);
//! ```

pub mod assertions;
pub mod codegen;
pub mod graphql;
pub mod http;
pub mod mock;
pub mod model;
pub mod oauth2;
pub mod postman;
pub mod runner;
pub mod samples;
pub mod scripts;
pub mod storage;
pub mod vars;
pub mod ws;

pub use model::{Auth, Body, Collection, Environment, HttpMethod, Item, KeyValue, Request};

/// Crate version, shown in the UI "About" dialog.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
