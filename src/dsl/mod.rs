//! Lossless parsing and loading for Crump's small declarative language.

mod parser;
mod syntax;

pub use parser::{ImportDeclaration, ModuleDocument, ProjectDocument, parse_module, parse_project};
