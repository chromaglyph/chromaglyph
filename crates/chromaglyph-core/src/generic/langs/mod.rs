//! Built-in language specs, one file per language. Adding a language:
//! new `langs/<name>.rs` with `spec()` + `samples()`, a `pub mod` line
//! here, a `spec_by_name` arm and a re-export in `../mod.rs`.

pub mod c;
pub mod cpp;
pub mod csharp;
pub mod go;
pub mod java;
pub mod javascript;
pub mod python;
pub mod rust;
pub mod typescript;
