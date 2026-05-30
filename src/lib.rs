//! atlas library — in-memory corpus graph.
//!
//! Exposes parsers, model, and graph loading for use by tests and
//! potentially by other tools in the wintermute ecosystem.
//!
//! **Read-only invariant:** atlas never writes any file in the corpus.

#![warn(missing_docs)]

pub mod args;
pub mod doctor;
pub mod edges;
pub mod graph;
pub mod model;
pub mod output;
pub mod parsers;
